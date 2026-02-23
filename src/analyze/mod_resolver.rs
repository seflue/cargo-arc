//! Filesystem-based module resolution for Rust crates.
//!
//! Resolves `mod` declarations to file paths, discovers crate root files,
//! and extracts module structure from the filesystem.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// A declared `mod` item (external, not inline).
pub(crate) struct ModDecl {
    pub(crate) name: String,
    pub(crate) explicit_path: Option<String>,
}

/// Check whether attributes contain `#[cfg(test)]`, including compound
/// expressions like `#[cfg(all(test, feature = "..."))]`.
pub(crate) fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    fn meta_contains_test(meta: &syn::Meta) -> bool {
        use syn::parse::Parser;
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
                let parser =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated;
                parser
                    .parse2(list.tokens.clone())
                    .is_ok_and(|nested| nested.iter().any(meta_contains_test))
            }
            _ => false,
        }
    }

    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<syn::Meta>()
            .is_ok_and(|meta| meta_contains_test(&meta))
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

/// Extract external `mod` declarations from a parsed syntax tree,
/// filtering out `#[cfg(test)]` modules (unless included) and inline modules.
pub(crate) fn extract_mod_declarations(syntax: &syn::File, include_tests: bool) -> Vec<ModDecl> {
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

/// Find root source files (lib.rs and/or main.rs) for a crate.
/// Returns all existing root files, lib.rs first.
/// Returns empty Vec (not error) when src/ is missing but tests/ exists (test-only crate).
pub(crate) fn find_crate_root_files(crate_path: &Path) -> Result<Vec<PathBuf>> {
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

/// Determine the directory where child modules are resolved.
/// dir-style files (lib.rs, main.rs, mod.rs): same directory.
/// file-style files (foo.rs): subdirectory foo/.
pub(crate) fn child_resolve_dir(file_path: &Path) -> PathBuf {
    let dir = file_path.parent().unwrap_or(Path::new("."));
    let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if file_name == "mod.rs" || file_name == "lib.rs" || file_name == "main.rs" {
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

    mod find_crate_root {
        use super::*;

        #[test]
        fn test_find_crate_root_lib() {
            let tmp = TestProject::new().file("src/lib.rs", "").build();
            let result = find_crate_root_files(tmp.path()).unwrap();
            assert_eq!(result, vec![tmp.path().join("src/lib.rs")]);
        }

        #[test]
        fn test_find_crate_root_main() {
            let tmp = TestProject::new().file("src/main.rs", "").build();
            let result = find_crate_root_files(tmp.path()).unwrap();
            assert_eq!(result, vec![tmp.path().join("src/main.rs")]);
        }

        #[test]
        fn test_find_crate_root_both_returns_vec() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "")
                .file("src/main.rs", "")
                .build();
            let result = find_crate_root_files(tmp.path()).unwrap();
            assert_eq!(
                result,
                vec![
                    tmp.path().join("src/lib.rs"),
                    tmp.path().join("src/main.rs")
                ]
            );
        }

        #[test]
        fn test_find_crate_root_missing() {
            let tmp = TestProject::new().build();
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
