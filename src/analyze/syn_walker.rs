//! Module discovery via syn + filesystem walk.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::mod_resolver::{
    CfgCondition, ModDecl, ModuleFile, declared_files, extract_mod_declarations,
};
use super::name_resolution::{FileInModule, NameResolution, PackageResolver, Root};
use super::packages::{ModulePaths, normalize_crate_name};
use crate::model::{EdgeContext, ModuleInfo, ModuleTree, PackageInfo, TestKind};

/// Find integration test files in `tests/*.rs`.
/// Each top-level `.rs` file is an independent test binary.
/// Subdirectory modules (e.g. `tests/common/mod.rs`) are NOT included.
fn find_integration_test_files(crate_path: &Path) -> Vec<PathBuf> {
    let tests_dir = crate_path.join("tests");
    if !tests_dir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(&tests_dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files.sort(); // deterministic order
    files
}

/// Read and parse the Rust source file at `file_path`.
fn parse_source(file_path: &Path) -> Result<syn::File> {
    let source = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    syn::parse_file(&source).with_context(|| format!("parsing {}", file_path.display()))
}

/// Parse a Rust source file and return all external `mod` declarations.
/// Convenience wrapper around `extract_mod_declarations` for callers with a file path.
#[cfg(test)]
fn parse_mod_declarations(file_path: &Path, include_tests: bool) -> Result<Vec<ModDecl>> {
    Ok(extract_mod_declarations(
        &parse_source(file_path)?,
        include_tests,
    ))
}

// ---------------------------------------------------------------------------
// Public API: collect_syn_module_paths
// ---------------------------------------------------------------------------

/// Recursively walk `mod` declarations starting from `module`,
/// collecting relative module paths (e.g. `"foo"`, `"foo::bar"`), those
/// declared inline with a body among them.
fn walk_modules_for_paths(
    module: &ModuleFile,
    parent_path: &str,
    modules: &mut ModulePaths,
    include_tests: bool,
) {
    let syntax = match parse_source(&module.path) {
        Ok(syntax) => syntax,
        Err(e) => {
            tracing::warn!("skipping {}: {e:#}", module.path.display());
            return;
        }
    };
    insert_inline_modules(&syntax.items, parent_path, modules, include_tests);

    for decl in extract_mod_declarations(&syntax, include_tests) {
        let child_full = if parent_path.is_empty() {
            decl.name.clone()
        } else {
            format!("{parent_path}::{}", decl.name)
        };
        modules.paths.insert(child_full.clone());

        for child in declared_files(&module.path, module.owns_directory, &decl) {
            walk_modules_for_paths(&child.file, &child_full, modules, include_tests);
        }
    }
}

/// Add the modules that `items`, written in the module at `parent_path`,
/// declare inline with a body, and the ones these declare in turn. A
/// `#[cfg(test)]` module only counts where test code is included, as for
/// `mod name;`.
fn insert_inline_modules(
    items: &[syn::Item],
    parent_path: &str,
    modules: &mut ModulePaths,
    include_tests: bool,
) {
    for item in items {
        let syn::Item::Mod(item_mod) = item else {
            continue;
        };
        let Some((_, content)) = &item_mod.content else {
            continue;
        };
        if !include_tests && CfgCondition::of_attrs(&item_mod.attrs).test_builds_only() {
            continue;
        }
        let path = if parent_path.is_empty() {
            item_mod.ident.to_string()
        } else {
            format!("{parent_path}::{}", item_mod.ident)
        };
        insert_inline_modules(content, &path, modules, include_tests);
        modules.paths.insert(path.clone());
        modules.inline.insert(path);
    }
}

/// The module paths of the integration test whose root is `test_file`.
fn test_crate_modules(test_file: &Path, include_tests: bool) -> ModulePaths {
    let root = ModuleFile {
        path: test_file.to_path_buf(),
        owns_directory: true,
    };
    let mut modules = ModulePaths::default();
    walk_modules_for_paths(&root, "", &mut modules, include_tests);
    modules
}

/// Collect all module paths reachable from the crate's targets via filesystem walk.
/// Returns relative paths without crate prefix, e.g. `{"analyze", "analyze::hir"}`,
/// with the inline modules among them.
pub(crate) fn collect_syn_module_paths(
    crate_info: &PackageInfo,
    include_tests: bool,
) -> ModulePaths {
    let mut modules = ModulePaths::default();
    for entry in &crate_info.entry_files {
        let root = ModuleFile {
            path: entry.file.clone(),
            owns_directory: true,
        };
        walk_modules_for_paths(&root, "", &mut modules, include_tests);
    }
    modules
}

// ---------------------------------------------------------------------------
// Public API: analyze_modules_syn
// ---------------------------------------------------------------------------

/// Invariant parameters shared across the recursive module walk.
#[derive(Clone)]
struct WalkContext<'a> {
    /// The crate root whose tree the walk reads.
    root: Root,
    /// The full path of the root's module: the package key, or for an
    /// integration test the full path of its root module.
    crate_name: &'a str,
    include_tests: bool,
    /// The context of the code the walk enters: test code for a module only
    /// test builds load, with all it holds.
    base_context: EdgeContext,
}

/// Take in the module read from another file of the same declaration
/// (`#[cfg_attr(…, path = …)]`): its dependencies, definitions and children,
/// a child of the same name merged in turn. The first file stays the module's.
fn merge_alternative(module: &mut ModuleInfo, alternative: ModuleInfo) {
    for dep in alternative.dependencies {
        if !module.dependencies.contains(&dep) {
            module.dependencies.push(dep);
        }
    }
    for (name, definition) in alternative.definitions {
        module.definitions.entry(name).or_insert(definition);
    }
    for child in alternative.children {
        match module.children.iter_mut().find(|c| c.name == child.name) {
            Some(known) => merge_alternative(known, child),
            None => module.children.push(child),
        }
    }
}

/// Walk the module `decl` declares in `parent`, every file it loads merged
/// into one. `None` when no file it names exists.
fn walk_declared_module(
    ctx: &WalkContext,
    parent: &ModuleFile,
    decl: &ModDecl,
    parent_path: &str,
    walked: &mut Walked<'_>,
) -> Option<ModuleInfo> {
    // A module declared under `#[cfg(test)]`, or a file only a test-only
    // `#[cfg_attr]` path loads, is test code with all it holds
    let test_ctx = WalkContext {
        base_context: ctx.base_context.promoted_to_test(),
        ..ctx.clone()
    };
    let mut alternatives = declared_files(&parent.path, parent.owns_directory, decl)
        .into_iter()
        .map(|declared| {
            let child_ctx =
                if decl.condition.test_builds_only() || declared.condition.test_builds_only() {
                    &test_ctx
                } else {
                    ctx
                };
            walk_module_syn(
                child_ctx,
                &declared.file.path,
                &decl.name,
                parent_path,
                declared.file.owns_directory,
                walked,
            )
        });
    let mut module = alternatives.next()?;
    for alternative in alternatives {
        merge_alternative(&mut module, alternative);
    }
    Some(module)
}

/// What the walk of a crate's module trees carries from file to file.
struct Walked<'w> {
    /// The files that do not parse.
    unparsed: Vec<PathBuf>,
    /// The resolution of the package's names, through all its roots.
    resolver: PackageResolver<'w>,
}

/// Recursively walk a module, building `ModuleInfo` with dependency
/// extraction. A file that does not parse is listed in `walked`.
fn walk_module_syn(
    ctx: &WalkContext,
    file_path: &Path,
    module_name: &str,
    parent_path: &str,
    owns_directory: bool,
    walked: &mut Walked<'_>,
) -> ModuleInfo {
    let full_path = if parent_path == module_name {
        // root module: full_path == crate name
        module_name.to_string()
    } else {
        format!("{parent_path}::{module_name}")
    };

    // Single read + single parse for both dependency extraction and mod discovery
    let source_text = match std::fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("reading {}: {e:#}", file_path.display());
            return ModuleInfo {
                name: module_name.to_string(),
                full_path,
                file: Some(file_path.to_path_buf()),
                children: Vec::new(),
                dependencies: Vec::new(),
                definitions: HashMap::new(),
            };
        }
    };
    let syntax = match syn::parse_file(&source_text) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("parsing {}: {e:#}", file_path.display());
            walked.unparsed.push(file_path.to_path_buf());
            return ModuleInfo {
                name: module_name.to_string(),
                full_path,
                file: Some(file_path.to_path_buf()),
                children: Vec::new(),
                dependencies: Vec::new(),
                definitions: HashMap::new(),
            };
        }
    };

    // Relative module path within the crate (e.g. "render" for render/mod.rs, "" for root)
    let current_module_path = full_path
        .strip_prefix(&format!("{}::", ctx.crate_name))
        .unwrap_or("");

    let pending = walked.resolver.dependencies(
        ctx.root,
        FileInModule {
            file: file_path,
            module_path: current_module_path,
            context: ctx.base_context.clone(),
        },
        &syntax,
    );

    // Extract mod declarations from the same AST (no second file read)
    let decls = extract_mod_declarations(&syntax, ctx.include_tests);

    let parent = ModuleFile {
        path: file_path.to_path_buf(),
        owns_directory,
    };
    let children: Vec<ModuleInfo> = decls
        .into_iter()
        .filter_map(|decl| walk_declared_module(ctx, &parent, &decl, &full_path, walked))
        .collect();

    // The descendants are walked now, so what they pass through this
    // module is known.
    let dependencies = walked.resolver.finish(pending);
    let definitions =
        walked
            .resolver
            .definitions(ctx.root, current_module_path, ctx.base_context.kind);

    ModuleInfo {
        name: module_name.to_string(),
        full_path,
        file: Some(file_path.to_path_buf()),
        children,
        dependencies,
        definitions,
    }
}

/// Walk the integration test whose root is `test_file` as a crate of its own,
/// keyed by the full path of its root module under `tests_path`. Cargo names
/// the crate as the file, `-` as `_`; the node shows the file's name.
fn walk_integration_test(
    tests_path: &str,
    test_file: &Path,
    include_tests: bool,
    walked: &mut Walked<'_>,
) -> ModuleInfo {
    let test_name = test_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let crate_name = normalize_crate_name(test_name);
    let test_crate = format!("{tests_path}::{crate_name}");
    let modules = test_crate_modules(test_file, include_tests);
    let test_ctx = WalkContext {
        root: walked.resolver.integration_test(&test_crate, modules),
        crate_name: &test_crate,
        include_tests,
        base_context: EdgeContext::test(TestKind::Integration),
    };
    let mut tree = walk_module_syn(&test_ctx, test_file, &crate_name, tests_path, true, walked);
    tree.name = test_name.to_string();
    tree
}

/// Analyze module hierarchy via syn + filesystem walk (no rust-analyzer needed).
/// Drop-in replacement for `hir::analyze_modules()`.
pub(crate) fn analyze_modules_syn(
    crate_info: &PackageInfo,
    resolution: &NameResolution,
    external_crate_names: &std::collections::HashMap<String, String>,
) -> ModuleTree {
    let normalized = normalize_crate_name(&crate_info.name);
    let include_tests = resolution.include_tests();
    let mut walked = Walked {
        unparsed: Vec::new(),
        resolver: resolution.package(crate_info, external_crate_names),
    };
    let mut root: Option<ModuleInfo> = None;
    for (index, entry) in crate_info.entry_files.iter().enumerate() {
        let root_ctx = WalkContext {
            root: Root::entry(index),
            crate_name: &normalized,
            include_tests,
            base_context: EdgeContext::production(),
        };
        let tree = walk_module_syn(
            &root_ctx,
            &entry.file,
            &normalized,
            &normalized, // parent_path == name for root → triggers identity check
            true,
            &mut walked,
        );
        match &mut root {
            None => root = Some(tree),
            Some(existing) => {
                for child in tree.children {
                    if !existing.children.iter().any(|c| c.name == child.name) {
                        existing.children.push(child);
                    }
                }
                // Dedup via PartialEq (compares kind + features). Correct while
                // every feature set is empty; once feature filtering lands,
                // entries differing only in features must be merged, not dropped.
                for dep in tree.dependencies {
                    if !existing.dependencies.contains(&dep) {
                        existing.dependencies.push(dep);
                    }
                }
            }
        }
    }

    // For test-only crates (no src/), create an empty root module
    if root.is_none() {
        root = Some(ModuleInfo {
            name: normalized.clone(),
            full_path: normalized.clone(),
            file: None,
            children: Vec::new(),
            dependencies: Vec::new(),
            definitions: HashMap::new(),
        });
    }

    // Walk integration test files (tests/*.rs) when --include-tests is active
    if include_tests {
        let test_files = find_integration_test_files(&crate_info.path);
        let root = root.as_mut().unwrap();
        let tests_path = format!("{normalized}::tests");
        for test_file in test_files {
            let tree = walk_integration_test(&tests_path, &test_file, include_tests, &mut walked);
            root.children.push(tree);
        }
    }

    let root = root.unwrap();
    ModuleTree {
        root,
        unparsed: walked.unparsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::analyze::packages::{ModulePathMap, WorkspacePackages};
    use crate::test_support::conventional_package;
    use tempfile::TempDir;

    struct TestProject {
        files: Vec<(PathBuf, String)>,
    }

    impl TestProject {
        fn new() -> Self {
            Self { files: vec![] }
        }

        fn file(mut self, path: &str, content: &str) -> Self {
            self.files.push((PathBuf::from(path), content.to_string()));
            self
        }

        fn build(self) -> TempDir {
            let tmp = TempDir::new().unwrap();
            for (path, content) in &self.files {
                let full = tmp.path().join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(&full, content).unwrap();
            }
            tmp
        }
    }

    mod parse_mod {
        use super::*;

        #[test]
        fn test_parse_mod_simple() {
            let tmp = TestProject::new().file("test.rs", "mod foo;").build();
            let path = tmp.path().join("test.rs");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "foo");
            assert!(decls[0].explicit_path.is_none());
        }

        #[test]
        fn test_parse_mod_cfg_test_filtered() {
            let tmp = TestProject::new()
                .file("test.rs", "#[cfg(test)]\nmod tests;")
                .build();
            let path = tmp.path().join("test.rs");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert!(decls.is_empty());
        }

        #[test]
        fn test_parse_mod_multiple() {
            let tmp = TestProject::new()
                .file("test.rs", "mod alpha;\nmod beta;\nmod gamma;")
                .build();
            let path = tmp.path().join("test.rs");

            let decls = parse_mod_declarations(&path, false).unwrap();
            let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
            assert_eq!(names, vec!["alpha", "beta", "gamma"]);
        }

        #[test]
        fn test_parse_mod_inline_ignored() {
            let tmp = TestProject::new()
                .file("test.rs", "mod foo { fn bar() {} }")
                .build();
            let path = tmp.path().join("test.rs");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert!(decls.is_empty());
        }

        #[test]
        fn test_parse_mod_with_path_attribute() {
            let tmp = TestProject::new()
                .file("test.rs", "#[path = \"custom.rs\"]\nmod foo;")
                .build();
            let path = tmp.path().join("test.rs");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "foo");
            assert_eq!(decls[0].explicit_path.as_deref(), Some("custom.rs"));
        }

        #[test]
        fn test_parse_mod_cfg_alternatives_read_once() {
            let tmp = TestProject::new()
                .file(
                    "test.rs",
                    "#[cfg(not(feature = \"x\"))]\nmod foo;\n#[cfg(feature = \"x\")]\npub mod foo;",
                )
                .build();
            let path = tmp.path().join("test.rs");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert_eq!(decls.len(), 1);
        }

        #[test]
        fn test_parse_mod_cfg_alternatives_with_distinct_paths_kept() {
            let tmp = TestProject::new()
                .file(
                    "test.rs",
                    "#[cfg(unix)]\n#[path = \"unix.rs\"]\nmod sys;\n#[cfg(windows)]\n#[path = \"windows.rs\"]\nmod sys;",
                )
                .build();
            let path = tmp.path().join("test.rs");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert_eq!(decls.len(), 2);
        }
    }

    mod collect_paths {
        use super::*;

        #[test]
        fn test_collect_paths_synthetic() {
            // lib.rs → mod foo; → foo.rs → mod bar; → foo/bar.rs
            let tmp = TestProject::new()
                .file("src/lib.rs", "mod foo;")
                .file("src/foo.rs", "mod bar;")
                .file("src/foo/bar.rs", "")
                .build();

            let paths =
                collect_syn_module_paths(&conventional_package("synth", tmp.path()), false).paths;
            assert!(
                paths.contains("foo"),
                "should contain 'foo', found: {paths:?}"
            );
            assert!(
                paths.contains("foo::bar"),
                "should contain 'foo::bar', found: {paths:?}"
            );
            assert_eq!(paths.len(), 2);
        }

        #[test]
        fn test_collect_paths_own_crate() {
            let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let paths =
                collect_syn_module_paths(&conventional_package("cargo_arc", crate_root), false)
                    .paths;

            for expected in [
                "analyze",
                "cli",
                "model",
                "graph",
                "analyze::hir",
                "analyze::use_parser",
            ] {
                assert!(
                    paths.contains(expected),
                    "should contain '{expected}', found: {paths:?}"
                );
            }
            // Must NOT contain crate prefix
            assert!(
                !paths.iter().any(|p| p.starts_with("cargo_arc::")),
                "paths should be relative, found: {paths:?}"
            );
        }

        #[test]
        fn test_collect_paths_empty_crate() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "// empty crate")
                .build();

            let paths =
                collect_syn_module_paths(&conventional_package("empty", tmp.path()), false).paths;
            assert!(paths.is_empty(), "expected empty set, found: {paths:?}");
        }

        #[test]
        fn test_mixed_crate_module_paths() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "mod a;")
                .file("src/main.rs", "mod b;")
                .file("src/a.rs", "")
                .file("src/b.rs", "")
                .build();

            let paths =
                collect_syn_module_paths(&conventional_package("mixed", tmp.path()), false).paths;
            assert!(paths.contains("a"), "should contain 'a', found: {paths:?}");
            assert!(paths.contains("b"), "should contain 'b', found: {paths:?}");
            assert_eq!(paths.len(), 2);
        }
    }

    mod analyze_syn {
        use super::*;

        /// A module whose file does not parse keeps its node, without
        /// children or dependencies, and the tree names the file.
        #[test]
        fn a_file_that_does_not_parse_is_listed_as_unparsed() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "mod good;\nmod broken;")
                .file("src/good.rs", "pub fn fine() {}")
                .file("src/broken.rs", "pub fn half(")
                .build();

            let tree = analyze_modules_syn(
                &conventional_package("fixture", tmp.path()),
                &NameResolution::syn_walk_only(
                    WorkspacePackages::default(),
                    &ModulePathMap::default(),
                    false,
                ),
                &std::collections::HashMap::new(),
            );

            assert_eq!(tree.unparsed, vec![tmp.path().join("src/broken.rs")]);
            let names: Vec<&str> = tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert_eq!(names, ["good", "broken"]);
        }

        #[test]
        fn test_analyze_modules_syn_structure() {
            let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let crate_info = conventional_package("cargo-arc", crate_root);
            let workspace_crates: WorkspacePackages = ["cargo-arc"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(workspace_crates, &ModulePathMap::default(), false),
                &std::collections::HashMap::new(),
            );

            assert_eq!(tree.root.name, "cargo_arc");
            assert_eq!(tree.root.full_path, "cargo_arc");

            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                child_names.contains(&"analyze"),
                "should contain 'analyze', found: {child_names:?}"
            );
            assert!(
                child_names.contains(&"graph"),
                "should contain 'graph', found: {child_names:?}"
            );

            // Check submodule hierarchy
            let analyze_mod = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "analyze")
                .unwrap();
            let sub_names: Vec<&str> = analyze_mod
                .children
                .iter()
                .map(|m| m.name.as_str())
                .collect();
            assert!(
                sub_names.contains(&"hir"),
                "analyze should contain 'hir', found: {sub_names:?}"
            );
            assert!(
                sub_names.contains(&"use_parser"),
                "analyze should contain 'use_parser', found: {sub_names:?}"
            );
        }

        #[test]
        fn test_analyze_modules_syn_dependencies() {
            let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let crate_info = conventional_package("cargo-arc", crate_root);
            let workspace_crates: WorkspacePackages = ["cargo-arc"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect();

            // Collect module paths for accurate dependency resolution
            let paths =
                collect_syn_module_paths(&conventional_package("cargo_arc", crate_root), false);
            let all_module_paths: ModulePathMap =
                [("cargo_arc".to_string(), paths)].into_iter().collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(workspace_crates, &all_module_paths, false),
                &std::collections::HashMap::new(),
            );

            let graph_mod = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "graph")
                .unwrap();
            assert!(
                graph_mod
                    .dependencies
                    .iter()
                    .any(|d| d.module_target() == "cargo_arc::model"),
                "graph should depend on model, found: {:?}",
                graph_mod.dependencies
            );
        }

        #[test]
        fn test_binary_only_crate() {
            let tmp = TestProject::new()
                .file("src/main.rs", "mod cli;")
                .file("src/cli.rs", "")
                .build();

            // collect_syn_module_paths finds "cli"
            let paths =
                collect_syn_module_paths(&conventional_package("binonly", tmp.path()), false).paths;
            assert!(
                paths.contains("cli"),
                "should contain 'cli', found: {paths:?}"
            );

            // analyze_modules_syn builds tree with "cli" child
            let crate_info = conventional_package("binonly", tmp.path());
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(
                    WorkspacePackages::default(),
                    &ModulePathMap::default(),
                    false,
                ),
                &std::collections::HashMap::new(),
            );

            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                child_names.contains(&"cli"),
                "should contain 'cli', found: {child_names:?}"
            );
        }

        #[test]
        fn test_module_carries_the_file_it_was_read_from() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "pub mod store;")
                .file("src/store/mod.rs", "pub mod disk;")
                .file("src/store/disk.rs", "")
                .build();

            let crate_info = conventional_package("fixture", tmp.path());
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(
                    WorkspacePackages::default(),
                    &ModulePathMap::default(),
                    false,
                ),
                &std::collections::HashMap::new(),
            );

            assert_eq!(tree.root.file, Some(tmp.path().join("src/lib.rs")));
            let store = &tree.root.children[0];
            assert_eq!(store.file, Some(tmp.path().join("src/store/mod.rs")));
            assert_eq!(
                store.children[0].file,
                Some(tmp.path().join("src/store/disk.rs"))
            );
        }

        #[test]
        fn test_path_ref_dependencies_collected() {
            let tmp = TestProject::new()
                .file(
                    "src/main.rs",
                    r"
fn main() {
    other_crate::module::run();
    let _x: other_crate::module::Config = todo!();
}
",
                )
                .build();

            let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
            let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
                .into_iter()
                .collect();
            let crate_info = PackageInfo {
                dependencies: vec!["other_crate".to_string()],
                ..conventional_package("app", tmp.path())
            };
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(ws, &mp, false),
                &std::collections::HashMap::new(),
            );

            // Path expressions should be detected as dependencies
            assert!(
                tree.root
                    .dependencies
                    .iter()
                    .any(|d| d.target_package == "other_crate" && d.target_module == "module"),
                "should detect path-ref dependency on other_crate::module, found: {:?}",
                tree.root.dependencies
            );
        }

        #[test]
        fn test_extern_crate_alias_in_root_reaches_child_modules() {
            let tmp = TestProject::new()
                .file(
                    "src/lib.rs",
                    "extern crate other_crate as oc;\npub mod user;\n",
                )
                .file("src/user.rs", "use oc::module::Item;\n")
                .build();

            let ws: WorkspacePackages = ["app", "other_crate"].into_iter().collect();
            let mp: ModulePathMap = [
                ("app".to_string(), HashSet::from(["user".into()])),
                ("other_crate".to_string(), HashSet::from(["module".into()])),
            ]
            .into_iter()
            .collect();
            let crate_info = PackageInfo {
                dependencies: vec!["other_crate".to_string()],
                ..conventional_package("app", tmp.path())
            };
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(ws, &mp, false),
                &std::collections::HashMap::new(),
            );

            let user = tree
                .root
                .children
                .iter()
                .find(|c| c.name == "user")
                .expect("user module");
            assert!(
                user.dependencies
                    .iter()
                    .any(|d| d.target_package == "other_crate" && d.target_module == "module"),
                "found: {:?}",
                user.dependencies
            );
        }

        #[test]
        fn test_path_ref_dedup_with_use() {
            let tmp = TestProject::new()
                .file(
                    "src/main.rs",
                    r"
use other_crate::module::Item;
fn main() {
    other_crate::module::Item::new();
}
",
                )
                .build();

            let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
            let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
                .into_iter()
                .collect();
            let crate_info = PackageInfo {
                dependencies: vec!["other_crate".to_string()],
                ..conventional_package("app", tmp.path())
            };
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(ws, &mp, false),
                &std::collections::HashMap::new(),
            );

            // Should have exactly 1 dep for other_crate::module::Item (deduped)
            let item_deps: Vec<_> = tree
                .root
                .dependencies
                .iter()
                .filter(|d| {
                    d.target_package == "other_crate"
                        && d.target_module == "module"
                        && d.target_item == Some("Item".to_string())
                })
                .collect();
            assert_eq!(
                item_deps.len(),
                1,
                "same target should be deduped, found: {:?}",
                tree.root.dependencies
            );
        }

        #[test]
        fn test_pub_use_marked_via_reexport() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "mod parent;\nmod child;\n")
                .file("src/parent.rs", "pub use crate::child::Thing;\n")
                .file("src/child.rs", "pub struct Thing;\n")
                .build();
            let mp: ModulePathMap = [(
                "my_crate".to_string(),
                HashSet::from(["parent".into(), "child".into()]),
            )]
            .into_iter()
            .collect();
            let crate_info = conventional_package("my_crate", tmp.path());
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(WorkspacePackages::default(), &mp, false),
                &std::collections::HashMap::new(),
            );

            let parent = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "parent")
                .expect("parent module should exist");
            let dep = parent
                .dependencies
                .iter()
                .find(|d| d.module_target() == "my_crate::child")
                .expect("parent should depend on child");
            assert!(
                dep.via_reexport,
                "pub use should be marked via_reexport, found: {dep:?}"
            );
        }

        #[test]
        fn test_plain_use_not_via_reexport() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "mod parent;\nmod child;\n")
                .file("src/parent.rs", "use crate::child::Thing;\n")
                .file("src/child.rs", "pub struct Thing;\n")
                .build();
            let mp: ModulePathMap = [(
                "my_crate".to_string(),
                HashSet::from(["parent".into(), "child".into()]),
            )]
            .into_iter()
            .collect();
            let crate_info = conventional_package("my_crate", tmp.path());
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(WorkspacePackages::default(), &mp, false),
                &std::collections::HashMap::new(),
            );

            let parent = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "parent")
                .expect("parent module should exist");
            let dep = parent
                .dependencies
                .iter()
                .find(|d| d.module_target() == "my_crate::child")
                .expect("parent should depend on child");
            assert!(
                !dep.via_reexport,
                "plain use should not be via_reexport, found: {dep:?}"
            );
        }

        #[test]
        fn test_mixed_crate_module_tree() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "mod a;")
                .file("src/main.rs", "mod b;")
                .file("src/a.rs", "")
                .file("src/b.rs", "")
                .build();

            let crate_info = conventional_package("mixed", tmp.path());
            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(
                    WorkspacePackages::default(),
                    &ModulePathMap::default(),
                    false,
                ),
                &std::collections::HashMap::new(),
            );

            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                child_names.contains(&"a"),
                "should contain 'a' from lib.rs, found: {child_names:?}"
            );
            assert!(
                child_names.contains(&"b"),
                "should contain 'b' from main.rs, found: {child_names:?}"
            );
        }
    }

    mod child_module_resolution {
        use super::*;

        #[test]
        fn test_child_module_bare_use_resolved() {
            // cargo-arc analyzes itself: render module must have deps on render::css and render::elements
            let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let crate_info = conventional_package("cargo-arc", crate_root);
            let workspace_crates: WorkspacePackages = ["cargo-arc"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect();
            let paths =
                collect_syn_module_paths(&conventional_package("cargo_arc", crate_root), false);
            let all_module_paths: ModulePathMap =
                [("cargo_arc".to_string(), paths)].into_iter().collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(workspace_crates, &all_module_paths, false),
                &std::collections::HashMap::new(),
            );

            let render_mod = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "render")
                .expect("should find render module");

            let dep_targets: Vec<String> = render_mod
                .dependencies
                .iter()
                .map(crate::model::DependencyRef::module_target)
                .collect();

            assert!(
                dep_targets.iter().any(|t| t == "cargo_arc::render::css"),
                "render should depend on render::css, found: {dep_targets:?}"
            );
            assert!(
                dep_targets
                    .iter()
                    .any(|t| t == "cargo_arc::render::elements"),
                "render should depend on render::elements, found: {dep_targets:?}"
            );
        }
    }

    mod find_integration_tests {
        use super::*;

        #[test]
        fn test_find_integration_test_files() {
            let tmp = TestProject::new()
                .file("tests/smoke.rs", "")
                .file("tests/check.rs", "")
                .file("tests/common/mod.rs", "")
                .build();

            let files = find_integration_test_files(tmp.path());
            let names: Vec<&str> = files
                .iter()
                .filter_map(|p| p.file_stem()?.to_str())
                .collect();
            assert!(names.contains(&"smoke"), "should contain smoke: {names:?}");
            assert!(names.contains(&"check"), "should contain check: {names:?}");
            assert_eq!(
                files.len(),
                2,
                "should not include common/mod.rs: {names:?}"
            );
        }

        #[test]
        fn test_find_integration_test_files_no_tests_dir() {
            let tmp = TestProject::new().build();
            let files = find_integration_test_files(tmp.path());
            assert!(files.is_empty());
        }
    }

    mod integration_test_analysis {
        use super::*;
        use crate::model::TestKind;

        #[test]
        fn test_analyze_crate_with_integration_tests() {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/integration_test_crate/crate_with_tests");
            let crate_info = PackageInfo {
                dependencies: vec!["crate_lib".to_string()],
                ..conventional_package("crate_with_tests", fixture)
            };
            let ws: WorkspacePackages = ["crate_with_tests", "crate_lib"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(ws, &ModulePathMap::default(), true),
                &std::collections::HashMap::new(),
            );

            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                child_names.contains(&"smoke"),
                "should contain integration test 'smoke': {child_names:?}"
            );

            let smoke = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "smoke")
                .unwrap();
            for dep in &smoke.dependencies {
                assert_eq!(
                    dep.context,
                    EdgeContext::test(TestKind::Integration),
                    "integration test deps should have Integration context: {dep:?}"
                );
            }
        }

        #[test]
        fn test_analyze_crate_without_include_tests_flag() {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/integration_test_crate/crate_with_tests");
            let crate_info = conventional_package("crate_with_tests", fixture);

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(
                    WorkspacePackages::default(),
                    &ModulePathMap::default(),
                    false,
                ),
                &std::collections::HashMap::new(),
            );

            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                !child_names.contains(&"smoke"),
                "without --include-tests, integration tests should not appear: {child_names:?}"
            );
        }

        #[test]
        fn test_analyze_test_only_crate() {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/integration_test_crate/test_only_crate");
            let crate_info = PackageInfo {
                dependencies: vec!["crate_lib".to_string()],
                ..conventional_package("test_only_crate", fixture)
            };
            let ws: WorkspacePackages = ["test_only_crate", "crate_lib"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(ws, &ModulePathMap::default(), true),
                &std::collections::HashMap::new(),
            );

            assert_eq!(tree.root.name, "test_only_crate");
            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                child_names.contains(&"check"),
                "test-only crate should have 'check' integration test: {child_names:?}"
            );
        }

        #[test]
        fn test_test_only_crate_is_empty_without_flag() {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/integration_test_crate/test_only_crate");
            let crate_info = conventional_package("test_only_crate", fixture);

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(
                    WorkspacePackages::default(),
                    &ModulePathMap::default(),
                    false,
                ),
                &std::collections::HashMap::new(),
            );
            assert!(tree.root.children.is_empty());
        }

        #[test]
        fn test_inline_cfg_test_deps_excluded_without_flag() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "mod alpha;\n")
                .file(
                    "src/alpha.rs",
                    r"
use crate::beta::helper;

pub fn process() {}

#[cfg(test)]
mod tests {
    use crate::gamma::test_util;
}
",
                )
                .build();

            let mp: ModulePathMap = [(
                "my_crate".to_string(),
                HashSet::from(["alpha".into(), "beta".into(), "gamma".into()]),
            )]
            .into_iter()
            .collect();
            let crate_info = conventional_package("my_crate", tmp.path());

            let tree = analyze_modules_syn(
                &crate_info,
                &NameResolution::syn_walk_only(WorkspacePackages::default(), &mp, false),
                &std::collections::HashMap::new(),
            );

            let alpha = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "alpha")
                .expect("alpha module should exist");

            let dep_modules: Vec<&str> = alpha
                .dependencies
                .iter()
                .map(|d| d.target_module.as_str())
                .collect();
            assert!(
                dep_modules.contains(&"beta"),
                "production dep on beta should be present: {dep_modules:?}"
            );
            assert!(
                !dep_modules.contains(&"gamma"),
                "test dep on gamma should be excluded without --include-tests: {dep_modules:?}"
            );
        }
    }
}
