//! Pass 1: the table of every binding in the workspace, built once before
//! the first package is resolved.

use std::collections::{HashMap, HashSet};

use super::super::packages::ModulePathMap;
#[cfg(test)]
use super::super::packages::{PackageExportMap, WorkspacePackages};
use super::super::reexports::{collect_reexport_map, name_a_fn_defines};
use super::super::use_parser::ReExportMap;
use super::package_resolver::{Backend, PackageResolver, WorkspaceView};
use crate::model::{EntryKind, PackageInfo};

/// The name resolution of a workspace: pass 1 runs when it is built, pass 2
/// for each package through [`NameResolution::package`].
pub(crate) struct NameResolution<'m>(WorkspaceView<'m>);

impl<'m> NameResolution<'m> {
    /// Build the table of `packages`, whose modules are `module_paths`. With
    /// `include_tests`, test code is resolved too.
    pub(crate) fn new(
        packages: &[PackageInfo],
        module_paths: &'m ModulePathMap,
        include_tests: bool,
    ) -> Self {
        let table = collect_reexport_map(packages, module_paths, include_tests);
        Self(WorkspaceView {
            table,
            include_tests,
            backend: Backend::Syn,
            ..Self::without_table(packages, module_paths).0
        })
    }

    /// The resolution of `packages` as the HIR backend runs it today: no
    /// table, so it stops where a path is written instead of following
    /// re-exports to the definer. It reads test code, takes one root per
    /// package and leaves each file's dependencies as resolved.
    pub(crate) fn without_table(packages: &[PackageInfo], module_paths: &'m ModulePathMap) -> Self {
        Self(WorkspaceView {
            workspace_crates: packages.iter().map(|p| p.name.clone()).collect(),
            module_paths,
            crate_exports: packages
                .iter()
                .map(|p| (p.key.clone(), collect_crate_exports(p)))
                .collect(),
            table: ReExportMap::default(),
            include_tests: true,
            backend: Backend::Hir,
        })
    }

    /// A syn walk over the workspace crates `workspace_crates` with neither a
    /// table nor package exports, for tests of what the walk does without
    /// resolving re-exports. Unlike [`NameResolution::without_table`], it
    /// resolves as the syn backend does and takes `include_tests`.
    #[cfg(test)]
    pub(crate) fn syn_walk_only(
        workspace_crates: WorkspacePackages,
        module_paths: &'m ModulePathMap,
        include_tests: bool,
    ) -> Self {
        Self(WorkspaceView {
            workspace_crates,
            module_paths,
            crate_exports: PackageExportMap::default(),
            table: ReExportMap::default(),
            include_tests,
            backend: Backend::Syn,
        })
    }

    /// Whether test code is resolved too.
    pub(crate) fn include_tests(&self) -> bool {
        self.0.include_tests
    }

    /// The resolution of `package`, which reaches external crates under the
    /// names `external_crate_names` gives.
    pub(crate) fn package<'w>(
        &'w self,
        package: &'w PackageInfo,
        external_crate_names: &'w HashMap<String, String>,
    ) -> PackageResolver<'w> {
        PackageResolver::new(&self.0, package, external_crate_names)
    }
}

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

/// Collect publicly exported symbol names from a crate's library target.
///
/// Includes:
/// - `pub fn`, `pub struct`, `pub enum`, `pub trait`, `pub const`, `pub static`, `pub type`
/// - a `#[proc_macro_derive(Name)]` fn under `Name`
/// - `pub use` re-exports (simple, aliased, grouped — NOT glob)
///
/// Ignores `pub mod` declarations (module structure, not exports).
/// Returns an empty set on any error (no library target, parse failure).
pub(crate) fn collect_crate_exports(crate_info: &PackageInfo) -> HashSet<String> {
    // Binary targets export nothing.
    let Some(root_file) = crate_info
        .entry_files
        .iter()
        .find(|entry| entry.kind == EntryKind::Lib)
        .map(|entry| &entry.file)
    else {
        return HashSet::new();
    };

    let Ok(source) = std::fs::read_to_string(root_file) else {
        return HashSet::new();
    };

    let Ok(syntax) = syn::parse_file(&source) else {
        return HashSet::new();
    };

    let mut exports = HashSet::new();

    for item in &syntax.items {
        match item {
            syn::Item::Fn(i) if is_pub(&i.vis) => {
                exports.insert(name_a_fn_defines(i).0.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::conventional_package;
    use tempfile::TempDir;

    /// A temporary directory holding `files`, each a path and its source.
    fn written(files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().expect("a temporary directory");
        for (path, content) in files {
            let full = tmp.path().join(path);
            std::fs::create_dir_all(full.parent().expect("a file sits in a directory"))
                .expect("the directory is writable");
            std::fs::write(&full, content).expect("the file is writable");
        }
        tmp
    }

    #[test]
    fn test_collect_exports_pub_items() {
        let tmp = written(&[(
            "src/lib.rs",
            r"
                pub fn helper() {}
                pub struct MyStruct;
                pub enum MyEnum { A, B }
                pub trait MyTrait {}
                pub const MAX: usize = 10;
                pub static GLOBAL: i32 = 0;
                pub type Alias = i32;
            ",
        )]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
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
        let tmp = written(&[("src/lib.rs", "pub use some_crate::Widget;\n")]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
        assert!(exports.contains("Widget"), "found: {exports:?}");
        assert_eq!(exports.len(), 1);
    }

    #[test]
    fn test_collect_exports_alias_reexport() {
        let tmp = written(&[("src/lib.rs", "pub use some_crate::Original as Alias;\n")]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
        assert!(exports.contains("Alias"), "found: {exports:?}");
        assert!(!exports.contains("Original"), "should not contain Original");
        assert_eq!(exports.len(), 1);
    }

    #[test]
    fn test_collect_exports_multi_reexport() {
        let tmp = written(&[("src/lib.rs", "pub use some_crate::{Alpha, Beta};\n")]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
        assert!(exports.contains("Alpha"), "found: {exports:?}");
        assert!(exports.contains("Beta"), "found: {exports:?}");
        assert_eq!(exports.len(), 2);
    }

    #[test]
    fn test_collect_exports_non_pub_ignored() {
        let tmp = written(&[(
            "src/lib.rs",
            r"
                fn private_fn() {}
                struct PrivateStruct;
                pub fn public_fn() {}
                pub(crate) fn crate_fn() {}
            ",
        )]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
        assert!(exports.contains("public_fn"), "found: {exports:?}");
        assert!(!exports.contains("private_fn"));
        assert!(!exports.contains("PrivateStruct"));
        assert!(!exports.contains("crate_fn"));
        assert_eq!(exports.len(), 1);
    }

    #[test]
    fn test_collect_exports_mod_ignored() {
        let tmp = written(&[("src/lib.rs", "pub mod foo;\npub fn real_export() {}\n")]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
        assert!(exports.contains("real_export"), "found: {exports:?}");
        assert!(!exports.contains("foo"), "pub mod should not be an export");
        assert_eq!(exports.len(), 1);
    }

    #[test]
    fn test_collect_exports_no_entry_file() {
        let tmp = written(&[]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
        assert!(exports.is_empty(), "found: {exports:?}");
    }

    #[test]
    fn test_mixed_crate_exports_only_lib() {
        let tmp = written(&[
            ("src/lib.rs", "pub fn from_lib() {}"),
            ("src/main.rs", "pub fn from_main() {}"),
        ]);

        let exports = collect_crate_exports(&conventional_package("fixture", tmp.path()));
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
