//! Module discovery via syn + filesystem walk.

use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::use_parser::{
    collect_all_path_refs, is_cfg_test, parse_path_ref_dependencies, parse_workspace_dependencies,
};
use crate::model::normalize_crate_name;
use crate::model::{
    CrateExportMap, CrateInfo, DependencyKind, DependencyRef, EdgeContext, ModuleInfo,
    ModulePathMap, ModuleTree, TestKind, WorkspaceCrates,
};

/// Find root source files (lib.rs and/or main.rs) for a crate.
/// Returns all existing root files, lib.rs first.
/// Returns empty Vec (not error) when src/ is missing but tests/ exists (test-only crate).
fn find_crate_root_files(crate_path: &Path) -> Result<Vec<PathBuf>> {
    let src = crate_path.join("src");
    let mut roots = Vec::new();
    let lib_rs = src.join("lib.rs");
    if lib_rs.exists() {
        roots.push(lib_rs);
    }
    let main_rs = src.join("main.rs");
    if main_rs.exists() {
        roots.push(main_rs);
    }
    if roots.is_empty() {
        // Test-only crates (no src/ but have tests/) are valid
        let tests_dir = crate_path.join("tests");
        if tests_dir.is_dir() {
            return Ok(roots);
        }
        bail!("no lib.rs or main.rs found in {}", src.display());
    }
    Ok(roots)
}

/// Find integration test files in `tests/*.rs`.
/// Each top-level `.rs` file is an independent test binary.
/// Subdirectory modules (e.g. `tests/common/mod.rs`) are NOT included.
fn find_integration_test_files(crate_path: &Path) -> Vec<PathBuf> {
    let tests_dir = crate_path.join("tests");
    if !tests_dir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(&tests_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
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

/// A declared `mod` item (external, not inline).
struct ModDecl {
    name: String,
    explicit_path: Option<String>,
}

/// Extract the value of a `#[path = "..."]` attribute, if present.
fn extract_path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("path") {
            return None;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            return Some(s.value());
        }
        None
    })
}

/// Extract external `mod` declarations from a parsed syntax tree,
/// filtering out `#[cfg(test)]` modules (unless included) and inline modules.
fn extract_mod_declarations(syntax: &syn::File, include_tests: bool) -> Vec<ModDecl> {
    let mut decls = Vec::new();
    for item in &syntax.items {
        if let syn::Item::Mod(item_mod) = item {
            if item_mod.content.is_some() {
                continue;
            }
            // Skip #[cfg(test)] modules unless --include-tests was passed
            if !include_tests && is_cfg_test(&item_mod.attrs) {
                continue;
            }
            decls.push(ModDecl {
                name: item_mod.ident.to_string(),
                explicit_path: extract_path_attribute(&item_mod.attrs),
            });
        }
    }
    decls
}

/// Parse a Rust source file and return all external `mod` declarations.
/// Convenience wrapper around `extract_mod_declarations` for callers with a file path.
fn parse_mod_declarations(file_path: &Path, include_tests: bool) -> Result<Vec<ModDecl>> {
    let source = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let syntax =
        syn::parse_file(&source).with_context(|| format!("parsing {}", file_path.display()))?;
    Ok(extract_mod_declarations(&syntax, include_tests))
}

/// Resolve a module name to its file path.
/// Checks `foo.rs` first, then `foo/mod.rs` (Rust 2018 convention).
fn resolve_mod_path(parent_dir: &Path, mod_name: &str) -> Option<PathBuf> {
    let file_path = parent_dir.join(format!("{mod_name}.rs"));
    if file_path.exists() {
        return Some(file_path);
    }
    let dir_path = parent_dir.join(mod_name).join("mod.rs");
    if dir_path.exists() {
        return Some(dir_path);
    }
    None
}

/// Determine the directory where child modules are resolved.
/// dir-style files (lib.rs, main.rs, mod.rs): same directory.
/// file-style files (foo.rs): subdirectory foo/.
fn child_resolve_dir(file_path: &Path) -> PathBuf {
    let dir = file_path.parent().unwrap_or(Path::new("."));
    let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if file_name == "mod.rs" || file_name == "lib.rs" || file_name == "main.rs" {
        dir.to_path_buf()
    } else {
        let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        dir.join(stem)
    }
}

// ---------------------------------------------------------------------------
// Public API: collect_syn_module_paths
// ---------------------------------------------------------------------------

/// Recursively walk `mod` declarations starting from `file_path`,
/// collecting relative module paths (e.g. `"foo"`, `"foo::bar"`).
fn walk_modules_for_paths(
    file_path: &Path,
    parent_path: &str,
    paths: &mut HashSet<String>,
    include_tests: bool,
) {
    let decls = match parse_mod_declarations(file_path, include_tests) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("skipping {}: {e:#}", file_path.display());
            return;
        }
    };

    let resolve_dir = child_resolve_dir(file_path);

    for decl in decls {
        let child_path = if let Some(ref explicit) = decl.explicit_path {
            let p = resolve_dir.join(explicit);
            if p.exists() { Some(p) } else { None }
        } else {
            resolve_mod_path(&resolve_dir, &decl.name)
        };

        let child_full = if parent_path.is_empty() {
            decl.name.clone()
        } else {
            format!("{parent_path}::{}", decl.name)
        };
        paths.insert(child_full.clone());

        if let Some(resolved) = child_path {
            walk_modules_for_paths(&resolved, &child_full, paths, include_tests);
        }
    }
}

/// Collect all module paths reachable from `crate_root` via filesystem walk.
/// Returns relative paths without crate prefix, e.g. `{"analyze", "analyze::hir"}`.
pub(crate) fn collect_syn_module_paths(
    crate_root: &Path,
    crate_name: &str,
    include_tests: bool,
) -> HashSet<String> {
    let _ = crate_name; // unused; kept for API parity with collect_hir_module_paths
    let root_files = match find_crate_root_files(crate_root) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("collect_syn_module_paths: {e:#}");
            return HashSet::new();
        }
    };
    let mut paths = HashSet::new();
    for root_file in root_files {
        walk_modules_for_paths(&root_file, "", &mut paths, include_tests);
    }
    paths
}

// ---------------------------------------------------------------------------
// Public API: collect_crate_exports
// ---------------------------------------------------------------------------

/// Extract the leaf name(s) from a `UseTree`.
/// Handles simple paths, aliases, and groups — but NOT globs.
fn collect_use_tree_names(tree: &syn::UseTree, names: &mut HashSet<String>) {
    match tree {
        syn::UseTree::Path(p) => collect_use_tree_names(&p.tree, names),
        syn::UseTree::Name(n) => {
            names.insert(n.ident.to_string());
        }
        syn::UseTree::Rename(r) => {
            names.insert(r.rename.to_string());
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                collect_use_tree_names(item, names);
            }
        }
        syn::UseTree::Glob(_) => {} // out of scope
    }
}

/// Returns whether a `syn::Visibility` is `pub` (not `pub(crate)`, `pub(super)`, etc.).
fn is_pub(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// Collect publicly exported symbol names from a crate's entry point (lib.rs/main.rs).
///
/// Includes:
/// - `pub fn`, `pub struct`, `pub enum`, `pub trait`, `pub const`, `pub static`, `pub type`
/// - `pub use` re-exports (simple, aliased, grouped — NOT glob)
///
/// Ignores `pub mod` declarations (module structure, not exports).
/// Returns an empty set on any error (no entry file, parse failure).
pub(crate) fn collect_crate_exports(crate_root: &Path) -> HashSet<String> {
    let root_files = match find_crate_root_files(crate_root) {
        Ok(f) => f,
        Err(_) => return HashSet::new(),
    };
    // Only lib.rs exports — binary targets export nothing
    let root_file = match root_files
        .iter()
        .find(|p| p.file_name().is_some_and(|n| n == "lib.rs"))
    {
        Some(f) => f,
        None => return HashSet::new(),
    };

    let source = match std::fs::read_to_string(root_file) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };

    let syntax = match syn::parse_file(&source) {
        Ok(f) => f,
        Err(_) => return HashSet::new(),
    };

    let mut exports = HashSet::new();

    for item in &syntax.items {
        match item {
            syn::Item::Fn(i) if is_pub(&i.vis) => {
                exports.insert(i.sig.ident.to_string());
            }
            syn::Item::Struct(i) if is_pub(&i.vis) => {
                exports.insert(i.ident.to_string());
            }
            syn::Item::Enum(i) if is_pub(&i.vis) => {
                exports.insert(i.ident.to_string());
            }
            syn::Item::Trait(i) if is_pub(&i.vis) => {
                exports.insert(i.ident.to_string());
            }
            syn::Item::Const(i) if is_pub(&i.vis) => {
                exports.insert(i.ident.to_string());
            }
            syn::Item::Static(i) if is_pub(&i.vis) => {
                exports.insert(i.ident.to_string());
            }
            syn::Item::Type(i) if is_pub(&i.vis) => {
                exports.insert(i.ident.to_string());
            }
            syn::Item::Use(i) if is_pub(&i.vis) => {
                collect_use_tree_names(&i.tree, &mut exports);
            }
            _ => {}
        }
    }

    exports
}

// ---------------------------------------------------------------------------
// Public API: analyze_modules_syn
// ---------------------------------------------------------------------------

/// Invariant parameters shared across the recursive module walk.
#[derive(Clone)]
struct WalkContext<'a> {
    crate_name: &'a str,
    crate_root: &'a Path,
    workspace_crates: &'a WorkspaceCrates,
    all_module_paths: &'a ModulePathMap,
    crate_exports: &'a CrateExportMap,
    include_tests: bool,
    base_context: EdgeContext,
}

/// Recursively walk a module, building `ModuleInfo` with dependency extraction.
fn walk_module_syn(
    ctx: &WalkContext,
    file_path: &Path,
    module_name: &str,
    parent_path: &str,
    is_crate_root: bool,
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
                children: Vec::new(),
                dependencies: Vec::new(),
            };
        }
    };
    let syntax = match syn::parse_file(&source_text) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("parsing {}: {e:#}", file_path.display());
            return ModuleInfo {
                name: module_name.to_string(),
                full_path,
                children: Vec::new(),
                dependencies: Vec::new(),
            };
        }
    };

    let source_file = file_path
        .strip_prefix(ctx.crate_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| file_path.to_path_buf());

    // Relative module path within the crate (e.g. "render" for render/mod.rs, "" for root)
    let current_module_path = full_path
        .strip_prefix(&format!("{}::", ctx.crate_name))
        .unwrap_or("");

    // Extract use items from all scopes (top-level + fn bodies + nested blocks)
    let use_items = super::use_parser::collect_all_use_items(&syntax, ctx.base_context.clone());
    let use_deps = parse_workspace_dependencies(
        &use_items,
        ctx.crate_name,
        ctx.workspace_crates,
        &source_file,
        ctx.all_module_paths,
        ctx.crate_exports,
        current_module_path,
    );

    // Extract qualified path references (e.g. my_lib::run(), let x: my_lib::Config)
    let path_refs = collect_all_path_refs(&syntax, ctx.base_context.clone());
    let path_deps = parse_path_ref_dependencies(
        &path_refs,
        ctx.crate_name,
        ctx.workspace_crates,
        &source_file,
        ctx.all_module_paths,
        ctx.crate_exports,
        current_module_path,
    );

    // Merge: use-dependencies first (have priority), then path-dependencies (dedup by (full_target, kind))
    let mut seen = DependencyRef::build_seen_index(&use_deps);
    let mut dependencies = use_deps;
    for dep in path_deps {
        DependencyRef::dedup_push(&mut dependencies, &mut seen, dep);
    }

    if !ctx.include_tests {
        dependencies.retain(|d| d.context.kind == DependencyKind::Production);
    }

    // Extract mod declarations from the same AST (no second file read)
    let decls = extract_mod_declarations(&syntax, ctx.include_tests);

    // Integration test files (tests/smoke.rs) are crate roots: resolve modules
    // from their parent directory (tests/), not from a stem-based subdirectory.
    let resolve_dir = if is_crate_root {
        file_path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        child_resolve_dir(file_path)
    };

    let children: Vec<ModuleInfo> = decls
        .into_iter()
        .filter_map(|decl| {
            let child_file = if let Some(ref explicit) = decl.explicit_path {
                let p = resolve_dir.join(explicit);
                if p.exists() { Some(p) } else { None }
            } else {
                resolve_mod_path(&resolve_dir, &decl.name)
            };

            child_file.map(|cf| walk_module_syn(ctx, &cf, &decl.name, &full_path, false))
        })
        .collect();

    ModuleInfo {
        name: module_name.to_string(),
        full_path,
        children,
        dependencies,
    }
}

/// Analyze module hierarchy via syn + filesystem walk (no rust-analyzer needed).
/// Drop-in replacement for `hir::analyze_modules()`.
pub(crate) fn analyze_modules_syn(
    crate_info: &CrateInfo,
    workspace_crates: &WorkspaceCrates,
    all_module_paths: &ModulePathMap,
    crate_exports: &CrateExportMap,
    include_tests: bool,
) -> Result<ModuleTree> {
    let root_files = find_crate_root_files(&crate_info.path)?;
    let normalized = normalize_crate_name(&crate_info.name);

    let ctx = WalkContext {
        crate_name: &normalized,
        crate_root: &crate_info.path,
        workspace_crates,
        all_module_paths,
        crate_exports,
        include_tests,
        base_context: EdgeContext::production(),
    };

    let mut root: Option<ModuleInfo> = None;
    for root_file in &root_files {
        let tree = walk_module_syn(
            &ctx,
            root_file,
            &normalized,
            &normalized, // parent_path == name for root → triggers identity check
            false,
        );
        match &mut root {
            None => root = Some(tree),
            Some(existing) => {
                for child in tree.children {
                    if !existing.children.iter().any(|c| c.name == child.name) {
                        existing.children.push(child);
                    }
                }
                // Dedup via PartialEq (compares kind + features). With empty
                // features this is correct; ca-0134 may need feature-merge here.
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
            children: Vec::new(),
            dependencies: Vec::new(),
        });
    }

    // Walk integration test files (tests/*.rs) when --include-tests is active
    if include_tests {
        let test_ctx = WalkContext {
            base_context: EdgeContext::test(TestKind::Integration),
            ..ctx.clone()
        };
        let test_files = find_integration_test_files(&crate_info.path);
        let root = root.as_mut().unwrap();
        for test_file in test_files {
            let test_name = test_file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let tree = walk_module_syn(
                &test_ctx,
                &test_file,
                test_name,
                &format!("{normalized}::tests"),
                true,
            );
            root.children.push(tree);
        }
    }

    let root = root.unwrap();
    Ok(ModuleTree { root })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    mod find_crate_root {
        use super::*;

        #[test]
        fn test_find_crate_root_lib() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "").unwrap();

            let result = find_crate_root_files(tmp.path()).unwrap();
            assert_eq!(result, vec![src.join("lib.rs")]);
        }

        #[test]
        fn test_find_crate_root_main() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("main.rs"), "").unwrap();

            let result = find_crate_root_files(tmp.path()).unwrap();
            assert_eq!(result, vec![src.join("main.rs")]);
        }

        #[test]
        fn test_find_crate_root_both_returns_vec() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "").unwrap();
            std::fs::write(src.join("main.rs"), "").unwrap();

            let result = find_crate_root_files(tmp.path()).unwrap();
            assert_eq!(result, vec![src.join("lib.rs"), src.join("main.rs")]);
        }

        #[test]
        fn test_find_crate_root_missing() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            let result = find_crate_root_files(tmp.path());
            assert!(result.is_err());
        }
    }

    mod is_cfg_test_tests {
        use super::*;

        fn parse_attrs(code: &str) -> Vec<syn::Attribute> {
            let file: syn::File = syn::parse_str(code).unwrap();
            match &file.items[0] {
                syn::Item::Mod(m) => m.attrs.clone(),
                _ => panic!("expected mod item"),
            }
        }

        #[test]
        fn test_is_cfg_test_positive() {
            let attrs = parse_attrs("#[cfg(test)] mod tests;");
            assert!(is_cfg_test(&attrs));
        }

        #[test]
        fn test_is_cfg_test_negative() {
            let attrs = parse_attrs("#[cfg(feature = \"foo\")] mod x;");
            assert!(!is_cfg_test(&attrs));
        }

        #[test]
        fn test_is_cfg_test_no_attrs() {
            let attrs = parse_attrs("mod foo;");
            assert!(!is_cfg_test(&attrs));
        }
    }

    mod parse_mod {
        use super::*;

        fn write_rust_file(tmp: &TempDir, content: &str) -> PathBuf {
            let path = tmp.path().join("test.rs");
            std::fs::write(&path, content).unwrap();
            path
        }

        #[test]
        fn test_parse_mod_simple() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "mod foo;");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "foo");
            assert!(decls[0].explicit_path.is_none());
        }

        #[test]
        fn test_parse_mod_cfg_test_filtered() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "#[cfg(test)]\nmod tests;");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert!(decls.is_empty());
        }

        #[test]
        fn test_parse_mod_multiple() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "mod alpha;\nmod beta;\nmod gamma;");

            let decls = parse_mod_declarations(&path, false).unwrap();
            let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
            assert_eq!(names, vec!["alpha", "beta", "gamma"]);
        }

        #[test]
        fn test_parse_mod_inline_ignored() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "mod foo { fn bar() {} }");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert!(decls.is_empty());
        }

        #[test]
        fn test_parse_mod_with_path_attribute() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "#[path = \"custom.rs\"]\nmod foo;");

            let decls = parse_mod_declarations(&path, false).unwrap();
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "foo");
            assert_eq!(decls[0].explicit_path.as_deref(), Some("custom.rs"));
        }
    }

    mod resolve_mod {
        use super::*;

        #[test]
        fn test_resolve_mod_file() {
            let tmp = TempDir::new().unwrap();
            std::fs::write(tmp.path().join("foo.rs"), "").unwrap();

            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, Some(tmp.path().join("foo.rs")));
        }

        #[test]
        fn test_resolve_mod_dir() {
            let tmp = TempDir::new().unwrap();
            let dir = tmp.path().join("foo");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("mod.rs"), "").unwrap();

            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, Some(dir.join("mod.rs")));
        }

        #[test]
        fn test_resolve_mod_missing() {
            let tmp = TempDir::new().unwrap();

            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, None);
        }

        #[test]
        fn test_resolve_mod_prefers_file() {
            let tmp = TempDir::new().unwrap();
            std::fs::write(tmp.path().join("foo.rs"), "").unwrap();
            let dir = tmp.path().join("foo");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("mod.rs"), "").unwrap();

            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, Some(tmp.path().join("foo.rs")));
        }
    }

    mod collect_paths {
        use super::*;

        #[test]
        fn test_collect_paths_synthetic() {
            // lib.rs → mod foo; → foo.rs → mod bar; → foo/bar.rs
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "mod foo;").unwrap();
            std::fs::write(src.join("foo.rs"), "mod bar;").unwrap();
            let foo_dir = src.join("foo");
            std::fs::create_dir_all(&foo_dir).unwrap();
            std::fs::write(foo_dir.join("bar.rs"), "").unwrap();

            let paths = collect_syn_module_paths(tmp.path(), "synth", false);
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
            let paths = collect_syn_module_paths(crate_root, "cargo_arc", false);

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
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "// empty crate").unwrap();

            let paths = collect_syn_module_paths(tmp.path(), "empty", false);
            assert!(paths.is_empty(), "expected empty set, found: {paths:?}");
        }

        #[test]
        fn test_mixed_crate_module_paths() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "mod a;").unwrap();
            std::fs::write(src.join("main.rs"), "mod b;").unwrap();
            std::fs::write(src.join("a.rs"), "").unwrap();
            std::fs::write(src.join("b.rs"), "").unwrap();

            let paths = collect_syn_module_paths(tmp.path(), "mixed", false);
            assert!(paths.contains("a"), "should contain 'a', found: {paths:?}");
            assert!(paths.contains("b"), "should contain 'b', found: {paths:?}");
            assert_eq!(paths.len(), 2);
        }
    }

    mod collect_exports {
        use super::*;

        #[test]
        fn test_collect_exports_pub_items() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("lib.rs"),
                r#"
                    pub fn helper() {}
                    pub struct MyStruct;
                    pub enum MyEnum { A, B }
                    pub trait MyTrait {}
                    pub const MAX: usize = 10;
                    pub static GLOBAL: i32 = 0;
                    pub type Alias = i32;
                "#,
            )
            .unwrap();

            let exports = collect_crate_exports(tmp.path());
            for name in [
                "helper", "MyStruct", "MyEnum", "MyTrait", "MAX", "GLOBAL", "Alias",
            ] {
                assert!(
                    exports.contains(name),
                    "should contain '{name}', found: {exports:?}"
                );
            }
            assert_eq!(exports.len(), 7);
        }

        #[test]
        fn test_collect_exports_reexports() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "pub use some_crate::Widget;\n").unwrap();

            let exports = collect_crate_exports(tmp.path());
            assert!(exports.contains("Widget"), "found: {exports:?}");
            assert_eq!(exports.len(), 1);
        }

        #[test]
        fn test_collect_exports_alias_reexport() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("lib.rs"),
                "pub use some_crate::Original as Alias;\n",
            )
            .unwrap();

            let exports = collect_crate_exports(tmp.path());
            assert!(exports.contains("Alias"), "found: {exports:?}");
            assert!(!exports.contains("Original"), "should not contain Original");
            assert_eq!(exports.len(), 1);
        }

        #[test]
        fn test_collect_exports_multi_reexport() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "pub use some_crate::{Alpha, Beta};\n").unwrap();

            let exports = collect_crate_exports(tmp.path());
            assert!(exports.contains("Alpha"), "found: {exports:?}");
            assert!(exports.contains("Beta"), "found: {exports:?}");
            assert_eq!(exports.len(), 2);
        }

        #[test]
        fn test_collect_exports_non_pub_ignored() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("lib.rs"),
                r#"
                    fn private_fn() {}
                    struct PrivateStruct;
                    pub fn public_fn() {}
                    pub(crate) fn crate_fn() {}
                "#,
            )
            .unwrap();

            let exports = collect_crate_exports(tmp.path());
            assert!(exports.contains("public_fn"), "found: {exports:?}");
            assert!(!exports.contains("private_fn"));
            assert!(!exports.contains("PrivateStruct"));
            assert!(!exports.contains("crate_fn"));
            assert_eq!(exports.len(), 1);
        }

        #[test]
        fn test_collect_exports_mod_ignored() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("lib.rs"),
                "pub mod foo;\npub fn real_export() {}\n",
            )
            .unwrap();

            let exports = collect_crate_exports(tmp.path());
            assert!(exports.contains("real_export"), "found: {exports:?}");
            assert!(!exports.contains("foo"), "pub mod should not be an export");
            assert_eq!(exports.len(), 1);
        }

        #[test]
        fn test_collect_exports_no_entry_file() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            // No lib.rs or main.rs

            let exports = collect_crate_exports(tmp.path());
            assert!(exports.is_empty(), "found: {exports:?}");
        }

        #[test]
        fn test_mixed_crate_exports_only_lib() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "pub fn from_lib() {}").unwrap();
            std::fs::write(src.join("main.rs"), "pub fn from_main() {}").unwrap();

            let exports = collect_crate_exports(tmp.path());
            assert!(
                exports.contains("from_lib"),
                "should contain 'from_lib', found: {exports:?}"
            );
            assert!(
                !exports.contains("from_main"),
                "should NOT contain 'from_main', found: {exports:?}"
            );
            assert_eq!(exports.len(), 1);
        }
    }

    mod analyze_syn {
        use super::*;

        #[test]
        fn test_analyze_modules_syn_structure() {
            let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let crate_info = CrateInfo {
                name: "cargo-arc".to_string(),
                path: crate_root.to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };
            let workspace_crates: WorkspaceCrates =
                ["cargo-arc"].iter().map(|s| s.to_string()).collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &workspace_crates,
                &ModulePathMap::default(),
                &CrateExportMap::default(),
                false,
            )
            .expect("should analyze");

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
            let crate_info = CrateInfo {
                name: "cargo-arc".to_string(),
                path: crate_root.to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };
            let workspace_crates: WorkspaceCrates =
                ["cargo-arc"].iter().map(|s| s.to_string()).collect();

            // Collect module paths for accurate dependency resolution
            let paths = collect_syn_module_paths(crate_root, "cargo_arc", false);
            let all_module_paths: ModulePathMap =
                [("cargo_arc".to_string(), paths)].into_iter().collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &workspace_crates,
                &all_module_paths,
                &CrateExportMap::default(),
                false,
            )
            .expect("should analyze");

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
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("main.rs"), "mod cli;").unwrap();
            std::fs::write(src.join("cli.rs"), "").unwrap();

            // collect_syn_module_paths finds "cli"
            let paths = collect_syn_module_paths(tmp.path(), "binonly", false);
            assert!(
                paths.contains("cli"),
                "should contain 'cli', found: {paths:?}"
            );

            // collect_crate_exports returns empty (no lib.rs)
            let exports = collect_crate_exports(tmp.path());
            assert!(
                exports.is_empty(),
                "binary-only should have no exports, found: {exports:?}"
            );

            // analyze_modules_syn builds tree with "cli" child
            let crate_info = CrateInfo {
                name: "binonly".to_string(),
                path: tmp.path().to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };
            let tree = analyze_modules_syn(
                &crate_info,
                &WorkspaceCrates::default(),
                &ModulePathMap::default(),
                &CrateExportMap::default(),
                false,
            )
            .expect("should analyze binary-only crate");

            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                child_names.contains(&"cli"),
                "should contain 'cli', found: {child_names:?}"
            );
        }

        #[test]
        fn test_path_ref_dependencies_collected() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            // main.rs uses qualified path expressions (no use-imports)
            std::fs::write(
                src.join("main.rs"),
                r#"
fn main() {
    other_crate::module::run();
    let _x: other_crate::module::Config = todo!();
}
"#,
            )
            .unwrap();

            let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
            let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
                .into_iter()
                .collect();
            let crate_info = CrateInfo {
                name: "app".to_string(),
                path: tmp.path().to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };
            let tree =
                analyze_modules_syn(&crate_info, &ws, &mp, &CrateExportMap::default(), false)
                    .expect("should analyze");

            // Path expressions should be detected as dependencies
            assert!(
                tree.root
                    .dependencies
                    .iter()
                    .any(|d| d.target_crate == "other_crate" && d.target_module == "module"),
                "should detect path-ref dependency on other_crate::module, found: {:?}",
                tree.root.dependencies
            );
        }

        #[test]
        fn test_path_ref_dedup_with_use() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            // main.rs has both a use-import and a qualified path for the same target
            std::fs::write(
                src.join("main.rs"),
                r#"
use other_crate::module::Item;
fn main() {
    other_crate::module::Item::new();
}
"#,
            )
            .unwrap();

            let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
            let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
                .into_iter()
                .collect();
            let crate_info = CrateInfo {
                name: "app".to_string(),
                path: tmp.path().to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };
            let tree =
                analyze_modules_syn(&crate_info, &ws, &mp, &CrateExportMap::default(), false)
                    .expect("should analyze");

            // Should have exactly 1 dep for other_crate::module::Item (deduped)
            let item_deps: Vec<_> = tree
                .root
                .dependencies
                .iter()
                .filter(|d| {
                    d.target_crate == "other_crate"
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
        fn test_mixed_crate_module_tree() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "mod a;").unwrap();
            std::fs::write(src.join("main.rs"), "mod b;").unwrap();
            std::fs::write(src.join("a.rs"), "").unwrap();
            std::fs::write(src.join("b.rs"), "").unwrap();

            let crate_info = CrateInfo {
                name: "mixed".to_string(),
                path: tmp.path().to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };
            let tree = analyze_modules_syn(
                &crate_info,
                &WorkspaceCrates::default(),
                &ModulePathMap::default(),
                &CrateExportMap::default(),
                false,
            )
            .expect("should analyze mixed crate");

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
            let crate_info = CrateInfo {
                name: "cargo-arc".to_string(),
                path: crate_root.to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };
            let workspace_crates: WorkspaceCrates =
                ["cargo-arc"].iter().map(|s| s.to_string()).collect();
            let paths = collect_syn_module_paths(crate_root, "cargo_arc", false);
            let all_module_paths: ModulePathMap =
                [("cargo_arc".to_string(), paths)].into_iter().collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &workspace_crates,
                &all_module_paths,
                &CrateExportMap::default(),
                false,
            )
            .expect("should analyze");

            let render_mod = tree
                .root
                .children
                .iter()
                .find(|m| m.name == "render")
                .expect("should find render module");

            let dep_targets: Vec<String> = render_mod
                .dependencies
                .iter()
                .map(|d| d.module_target())
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
            let tmp = TempDir::new().unwrap();
            let tests = tmp.path().join("tests");
            std::fs::create_dir_all(&tests).unwrap();
            std::fs::write(tests.join("smoke.rs"), "").unwrap();
            std::fs::write(tests.join("check.rs"), "").unwrap();
            let common = tests.join("common");
            std::fs::create_dir_all(&common).unwrap();
            std::fs::write(common.join("mod.rs"), "").unwrap();

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
            let tmp = TempDir::new().unwrap();
            let files = find_integration_test_files(tmp.path());
            assert!(files.is_empty());
        }

        #[test]
        fn test_find_crate_root_test_only_crate() {
            let tmp = TempDir::new().unwrap();
            let tests = tmp.path().join("tests");
            std::fs::create_dir_all(&tests).unwrap();
            std::fs::write(tests.join("check.rs"), "").unwrap();

            let roots = find_crate_root_files(tmp.path()).unwrap();
            assert!(
                roots.is_empty(),
                "test-only crate should return empty roots"
            );
        }

        #[test]
        fn test_find_crate_root_no_src_no_tests_errors() {
            let tmp = TempDir::new().unwrap();
            let result = find_crate_root_files(tmp.path());
            assert!(result.is_err());
        }
    }

    mod integration_test_analysis {
        use super::*;
        use crate::model::TestKind;

        #[test]
        fn test_analyze_crate_with_integration_tests() {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/integration_test_crate/crate_with_tests");
            let crate_info = CrateInfo {
                name: "crate_with_tests".to_string(),
                path: fixture,
                dependencies: vec!["crate_lib".to_string()],
                dev_dependencies: vec![],
            };
            let ws: WorkspaceCrates = ["crate_with_tests", "crate_lib"]
                .iter()
                .map(|s| s.to_string())
                .collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &ws,
                &ModulePathMap::default(),
                &CrateExportMap::default(),
                true,
            )
            .expect("should analyze");

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
            let crate_info = CrateInfo {
                name: "crate_with_tests".to_string(),
                path: fixture,
                dependencies: vec![],
                dev_dependencies: vec![],
            };

            let tree = analyze_modules_syn(
                &crate_info,
                &WorkspaceCrates::default(),
                &ModulePathMap::default(),
                &CrateExportMap::default(),
                false,
            )
            .expect("should analyze");

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
            let crate_info = CrateInfo {
                name: "test_only_crate".to_string(),
                path: fixture,
                dependencies: vec!["crate_lib".to_string()],
                dev_dependencies: vec![],
            };
            let ws: WorkspaceCrates = ["test_only_crate", "crate_lib"]
                .iter()
                .map(|s| s.to_string())
                .collect();

            let tree = analyze_modules_syn(
                &crate_info,
                &ws,
                &ModulePathMap::default(),
                &CrateExportMap::default(),
                true,
            )
            .expect("should analyze test-only crate");

            assert_eq!(tree.root.name, "test_only_crate");
            let child_names: Vec<&str> =
                tree.root.children.iter().map(|m| m.name.as_str()).collect();
            assert!(
                child_names.contains(&"check"),
                "test-only crate should have 'check' integration test: {child_names:?}"
            );
        }

        #[test]
        fn test_test_only_crate_errors_without_flag() {
            let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/integration_test_crate/test_only_crate");
            let crate_info = CrateInfo {
                name: "test_only_crate".to_string(),
                path: fixture,
                dependencies: vec![],
                dev_dependencies: vec![],
            };

            let tree = analyze_modules_syn(
                &crate_info,
                &WorkspaceCrates::default(),
                &ModulePathMap::default(),
                &CrateExportMap::default(),
                false,
            )
            .expect("should not error for test-only crate");
            assert!(tree.root.children.is_empty());
        }

        #[test]
        fn test_inline_cfg_test_deps_excluded_without_flag() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("lib.rs"),
                r#"
mod alpha;
"#,
            )
            .unwrap();
            std::fs::write(
                src.join("alpha.rs"),
                r#"
use crate::beta::helper;

pub fn process() {}

#[cfg(test)]
mod tests {
    use crate::gamma::test_util;
}
"#,
            )
            .unwrap();

            let mp: ModulePathMap = [(
                "my_crate".to_string(),
                HashSet::from(["alpha".into(), "beta".into(), "gamma".into()]),
            )]
            .into_iter()
            .collect();
            let crate_info = CrateInfo {
                name: "my_crate".to_string(),
                path: tmp.path().to_path_buf(),
                dependencies: vec![],
                dev_dependencies: vec![],
            };

            let tree = analyze_modules_syn(
                &crate_info,
                &WorkspaceCrates::default(),
                &mp,
                &CrateExportMap::default(),
                false,
            )
            .expect("should analyze");

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
