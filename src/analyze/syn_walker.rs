//! Module discovery via syn + filesystem walk.

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::use_parser::{normalize_crate_name, parse_workspace_dependencies};
use crate::model::{CrateInfo, ModuleInfo, ModuleTree};

/// Find the root source file (lib.rs or main.rs) for a crate.
/// Prefers lib.rs over main.rs when both exist.
fn find_crate_root_file(crate_path: &Path) -> Result<PathBuf> {
    let src = crate_path.join("src");
    let lib_rs = src.join("lib.rs");
    if lib_rs.exists() {
        return Ok(lib_rs);
    }
    let main_rs = src.join("main.rs");
    if main_rs.exists() {
        return Ok(main_rs);
    }
    bail!("no lib.rs or main.rs found in {}", src.display())
}

/// A declared `mod` item (external, not inline).
struct ModDecl {
    name: String,
    explicit_path: Option<String>,
}

/// Check whether attributes contain `#[cfg(test)]`.
fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                found = true;
            }
            Ok(())
        });
        found
    })
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

/// Parse a Rust source file and return all external `mod` declarations,
/// filtering out `#[cfg(test)]` modules (unless included) and inline modules.
fn parse_mod_declarations(file_path: &Path, include_cfg_test: bool) -> Result<Vec<ModDecl>> {
    let source = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let syntax =
        syn::parse_file(&source).with_context(|| format!("parsing {}", file_path.display()))?;

    let mut decls = Vec::new();
    for item in &syntax.items {
        if let syn::Item::Mod(item_mod) = item {
            // Skip inline modules (have a body)
            if item_mod.content.is_some() {
                continue;
            }
            // Skip #[cfg(test)] modules unless --cfg test was passed
            if !include_cfg_test && is_cfg_test(&item_mod.attrs) {
                continue;
            }
            decls.push(ModDecl {
                name: item_mod.ident.to_string(),
                explicit_path: extract_path_attribute(&item_mod.attrs),
            });
        }
    }
    Ok(decls)
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
    include_cfg_test: bool,
) {
    let decls = match parse_mod_declarations(file_path, include_cfg_test) {
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
            walk_modules_for_paths(&resolved, &child_full, paths, include_cfg_test);
        }
    }
}

/// Collect all module paths reachable from `crate_root` via filesystem walk.
/// Returns relative paths without crate prefix, e.g. `{"analyze", "analyze::hir"}`.
pub(crate) fn collect_syn_module_paths(
    crate_root: &Path,
    crate_name: &str,
    include_cfg_test: bool,
) -> HashSet<String> {
    let _ = crate_name; // unused; kept for API parity with collect_hir_module_paths
    let root_file = match find_crate_root_file(crate_root) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("collect_syn_module_paths: {e:#}");
            return HashSet::new();
        }
    };
    let mut paths = HashSet::new();
    walk_modules_for_paths(&root_file, "", &mut paths, include_cfg_test);
    paths
}

// ---------------------------------------------------------------------------
// Public API: collect_crate_exports
// ---------------------------------------------------------------------------

/// Extract the leaf name(s) from a `UseTree`.
/// Handles simple paths, aliases, and groups — but NOT globs.
#[allow(dead_code)] // Used in Phase 3 when crate_exports is wired through
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
#[allow(dead_code)] // Used in Phase 3 when crate_exports is wired through
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
#[allow(dead_code)] // Used in Phase 3 when crate_exports is wired through
pub(crate) fn collect_crate_exports(crate_root: &Path) -> HashSet<String> {
    let root_file = match find_crate_root_file(crate_root) {
        Ok(f) => f,
        Err(_) => return HashSet::new(),
    };

    let source = match std::fs::read_to_string(&root_file) {
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

/// Recursively walk a module, building `ModuleInfo` with dependency extraction.
#[allow(clippy::too_many_arguments)]
fn walk_module_syn(
    file_path: &Path,
    module_name: &str,
    parent_path: &str,
    crate_name: &str,
    crate_root: &Path,
    workspace_crates: &HashSet<String>,
    all_module_paths: &HashMap<String, HashSet<String>>,
    include_cfg_test: bool,
) -> ModuleInfo {
    let full_path = if parent_path == module_name {
        // root module: full_path == crate name
        module_name.to_string()
    } else {
        format!("{parent_path}::{module_name}")
    };

    // Read source for dependency extraction
    let source_text = match std::fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("reading {}: {e:#}", file_path.display());
            String::new()
        }
    };
    let source_file = file_path
        .strip_prefix(crate_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| file_path.to_path_buf());

    let dependencies = parse_workspace_dependencies(
        &source_text,
        crate_name,
        workspace_crates,
        &source_file,
        all_module_paths,
        &HashMap::new(),
    );

    // Discover children
    let decls = parse_mod_declarations(file_path, include_cfg_test).unwrap_or_default();

    let resolve_dir = child_resolve_dir(file_path);

    let children: Vec<ModuleInfo> = decls
        .into_iter()
        .filter_map(|decl| {
            let child_file = if let Some(ref explicit) = decl.explicit_path {
                let p = resolve_dir.join(explicit);
                if p.exists() { Some(p) } else { None }
            } else {
                resolve_mod_path(&resolve_dir, &decl.name)
            };

            child_file.map(|cf| {
                walk_module_syn(
                    &cf,
                    &decl.name,
                    &full_path,
                    crate_name,
                    crate_root,
                    workspace_crates,
                    all_module_paths,
                    include_cfg_test,
                )
            })
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
    workspace_crates: &HashSet<String>,
    all_module_paths: &HashMap<String, HashSet<String>>,
    include_cfg_test: bool,
) -> Result<ModuleTree> {
    let root_file = find_crate_root_file(&crate_info.path)?;
    let normalized = normalize_crate_name(&crate_info.name);

    let root = walk_module_syn(
        &root_file,
        &normalized,
        &normalized, // parent_path == name for root → triggers identity check
        &normalized,
        &crate_info.path,
        workspace_crates,
        all_module_paths,
        include_cfg_test,
    );

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

            let result = find_crate_root_file(tmp.path()).unwrap();
            assert_eq!(result, src.join("lib.rs"));
        }

        #[test]
        fn test_find_crate_root_main() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("main.rs"), "").unwrap();

            let result = find_crate_root_file(tmp.path()).unwrap();
            assert_eq!(result, src.join("main.rs"));
        }

        #[test]
        fn test_find_crate_root_prefers_lib() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("lib.rs"), "").unwrap();
            std::fs::write(src.join("main.rs"), "").unwrap();

            let result = find_crate_root_file(tmp.path()).unwrap();
            assert_eq!(result, src.join("lib.rs"));
        }

        #[test]
        fn test_find_crate_root_missing() {
            let tmp = TempDir::new().unwrap();
            let src = tmp.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            let result = find_crate_root_file(tmp.path());
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
            };
            let workspace_crates: HashSet<String> =
                ["cargo-arc"].iter().map(|s| s.to_string()).collect();

            let tree = analyze_modules_syn(&crate_info, &workspace_crates, &HashMap::new(), false)
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
            };
            let workspace_crates: HashSet<String> =
                ["cargo-arc"].iter().map(|s| s.to_string()).collect();

            // Collect module paths for accurate dependency resolution
            let mut all_module_paths = HashMap::new();
            let paths = collect_syn_module_paths(crate_root, "cargo_arc", false);
            all_module_paths.insert("cargo_arc".to_string(), paths);

            let tree =
                analyze_modules_syn(&crate_info, &workspace_crates, &all_module_paths, false)
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
    }
}
