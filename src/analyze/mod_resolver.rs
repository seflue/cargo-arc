//! Filesystem-based module resolution for Rust crates.
//!
//! Resolves `mod` declarations to file paths, discovers crate root files,
//! and extracts module structure from the filesystem.

use std::path::{Path, PathBuf};

/// A declared `mod` item (external, not inline).
pub(crate) struct ModDecl {
    pub(crate) name: String,
    pub(crate) explicit_path: Option<String>,
    /// The paths `#[cfg_attr(…, path = …)]` names. Each is an alternative to
    /// the file the declaration loads without it.
    pub(crate) cfg_attr_paths: Vec<String>,
    /// Whether every declaration of the module stands under `#[cfg(test)]`,
    /// which makes the module test code with all it holds.
    pub(crate) cfg_test: bool,
}

/// Check whether a `#[cfg]` admits the item into test builds only: `test`,
/// `all(..)` with `test` among its predicates, or `any(..)` whose every
/// predicate admits test builds only. `any(windows, test)` also builds
/// outside the test harness and is no test code.
pub(crate) fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    fn requires_test(meta: &syn::Meta) -> bool {
        use syn::parse::Parser;
        let nested = |list: &syn::MetaList| {
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
        };
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) if list.path.is_ident("all") => {
                nested(list).is_ok_and(|predicates| predicates.iter().any(requires_test))
            }
            syn::Meta::List(list) if list.path.is_ident("any") => {
                nested(list).is_ok_and(|predicates| {
                    !predicates.is_empty() && predicates.iter().all(requires_test)
                })
            }
            _ => false,
        }
    }

    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<syn::Meta>()
            .is_ok_and(|meta| requires_test(&meta))
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

/// Extract the values of `path = "..."` inside `#[cfg_attr(predicate, …)]`.
fn extract_cfg_attr_paths(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg_attr"))
        .filter_map(|attr| {
            attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .ok()
        })
        .flat_map(|metas| metas.into_iter().skip(1))
        .filter_map(|meta| match meta {
            syn::Meta::NameValue(nv) if nv.path.is_ident("path") => match nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Extract external `mod` declarations from a parsed syntax tree,
/// filtering out `#[cfg(test)]` modules (unless included) and inline modules.
pub(crate) fn extract_mod_declarations(syntax: &syn::File, include_tests: bool) -> Vec<ModDecl> {
    let mut decls: Vec<ModDecl> = Vec::new();
    for item in &syntax.items {
        if let syn::Item::Mod(item_mod) = item {
            if item_mod.content.is_some() {
                continue;
            }
            let cfg_test = is_cfg_test(&item_mod.attrs);
            // Skip #[cfg(test)] modules unless --include-tests was passed
            if !include_tests && cfg_test {
                continue;
            }
            let name = item_mod.ident.to_string();
            let explicit_path = extract_path_attribute(&item_mod.attrs);
            let cfg_attr_paths = extract_cfg_attr_paths(&item_mod.attrs);
            // `mod foo;` repeated under complementary cfgs names one file
            if let Some(known) = decls.iter_mut().find(|d| {
                d.name == name
                    && d.explicit_path == explicit_path
                    && d.cfg_attr_paths == cfg_attr_paths
            }) {
                known.cfg_test &= cfg_test;
                continue;
            }
            decls.push(ModDecl {
                name,
                explicit_path,
                cfg_attr_paths,
                cfg_test,
            });
        }
    }
    decls
}

/// Resolve a module name to its file path.
/// Checks `foo.rs` first, then `foo/mod.rs` (Rust 2018 convention).
pub(crate) fn resolve_mod_path(parent_dir: &Path, mod_name: &str) -> Option<PathBuf> {
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

/// A module's source file, and whether it owns its directory: its children
/// live beside it, as they do for a crate root or a file a `#[path]` names,
/// whatever the file's name.
pub(crate) struct ModuleFile {
    pub(crate) path: PathBuf,
    pub(crate) owns_directory: bool,
}

/// The files a `mod` declaration in `file_path` loads. A `#[path]`, plain or
/// in `#[cfg_attr]`, is relative to the directory of `file_path`, and the
/// file it names owns its directory. Each `#[cfg_attr(…, path = …)]` file is
/// an alternative to the file the declaration loads without it; a file that
/// does not exist is left out.
pub(crate) fn declared_files(
    file_path: &Path,
    owns_directory: bool,
    decl: &ModDecl,
) -> Vec<ModuleFile> {
    let dir = file_path.parent().unwrap_or(Path::new("."));
    let by_path = |path: &String| {
        let path = dir.join(path);
        path.exists().then_some(ModuleFile {
            path,
            owns_directory: true,
        })
    };
    let plain = match &decl.explicit_path {
        Some(path) => by_path(path),
        None => resolve_mod_path(&child_resolve_dir(file_path, owns_directory), &decl.name).map(
            |path| ModuleFile {
                path,
                owns_directory: false,
            },
        ),
    };
    let mut files: Vec<ModuleFile> = plain.into_iter().collect();
    for file in decl.cfg_attr_paths.iter().filter_map(by_path) {
        if !files.iter().any(|known| known.path == file.path) {
            files.push(file);
        }
    }
    files
}

/// Determine the directory where child modules are resolved.
/// dir-style files (lib.rs, main.rs, mod.rs): same directory.
/// file-style files (foo.rs): subdirectory foo/.
///
/// A file that owns its directory is always dir-style regardless of its file
/// name: `[lib] path` may name a crate root anything, and `#[path]` any
/// module's file, and their children still live beside them.
fn child_resolve_dir(file_path: &Path, owns_directory: bool) -> PathBuf {
    let dir = file_path.parent().unwrap_or(Path::new("."));
    let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if owns_directory || file_name == "mod.rs" || file_name == "lib.rs" || file_name == "main.rs" {
        dir.to_path_buf()
    } else {
        let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        dir.join(stem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    mod resolve_dir {
        use super::*;

        #[test]
        fn test_dir_style_files_resolve_in_place() {
            for name in ["lib.rs", "main.rs", "mod.rs"] {
                let path = PathBuf::from("/w/src").join(name);
                assert_eq!(child_resolve_dir(&path, false), PathBuf::from("/w/src"));
            }
        }

        #[test]
        fn test_file_style_resolves_into_subdir() {
            let path = PathBuf::from("/w/src/entry.rs");
            assert_eq!(
                child_resolve_dir(&path, false),
                PathBuf::from("/w/src/entry")
            );
        }

        #[test]
        fn test_crate_root_resolves_in_place_despite_name() {
            let path = PathBuf::from("/w/src/entry.rs");
            assert_eq!(child_resolve_dir(&path, true), PathBuf::from("/w/src"));
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

    mod mod_declarations {
        use super::*;

        #[test]
        fn test_module_is_test_code_only_where_every_declaration_is() {
            let syntax: syn::File = syn::parse_str(
                "#[cfg(test)] mod both; #[cfg(not(test))] mod both; #[cfg(test)] mod only;",
            )
            .unwrap();
            let decls = extract_mod_declarations(&syntax, true);
            let cfg_test: Vec<_> = decls
                .iter()
                .map(|d| (d.name.as_str(), d.cfg_test))
                .collect();
            assert_eq!(cfg_test, [("both", false), ("only", true)]);
        }

        #[test]
        fn test_cfg_attr_paths_are_read_and_other_cfg_attrs_ignored() {
            let syntax: syn::File = syn::parse_str(
                r#"#[cfg_attr(unix, path = "u.rs")] #[cfg_attr(windows, allow(dead_code), path = "w.rs")] mod imp;
                   #[cfg_attr(test, allow(dead_code))] mod plain;"#,
            )
            .unwrap();
            let decls = extract_mod_declarations(&syntax, true);
            let paths: Vec<_> = decls
                .iter()
                .map(|d| (d.name.as_str(), d.cfg_attr_paths.clone()))
                .collect();
            assert_eq!(
                paths,
                [
                    ("imp", vec!["u.rs".to_string(), "w.rs".to_string()]),
                    ("plain", vec![])
                ]
            );
        }
    }

    mod resolve_mod {
        use super::*;

        #[test]
        fn test_resolve_mod_file() {
            let tmp = TestProject::new().file("foo.rs", "").build();
            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, Some(tmp.path().join("foo.rs")));
        }

        #[test]
        fn test_resolve_mod_dir() {
            let tmp = TestProject::new().file("foo/mod.rs", "").build();
            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, Some(tmp.path().join("foo/mod.rs")));
        }

        #[test]
        fn test_resolve_mod_missing() {
            let tmp = TestProject::new().build();
            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, None);
        }

        #[test]
        fn test_resolve_mod_prefers_file() {
            let tmp = TestProject::new()
                .file("foo.rs", "")
                .file("foo/mod.rs", "")
                .build();
            let result = resolve_mod_path(tmp.path(), "foo");
            assert_eq!(result, Some(tmp.path().join("foo.rs")));
        }
    }
}
