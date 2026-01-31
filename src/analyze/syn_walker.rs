//! Module discovery via syn + filesystem walk.

#![allow(dead_code, unused_imports)] // Phase 1: internal functions, pub API comes in Phase 2

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

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
/// filtering out `#[cfg(test)]` modules and inline modules.
fn parse_mod_declarations(file_path: &Path) -> Result<Vec<ModDecl>> {
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
            // Skip #[cfg(test)] modules
            if is_cfg_test(&item_mod.attrs) {
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

            let decls = parse_mod_declarations(&path).unwrap();
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "foo");
            assert!(decls[0].explicit_path.is_none());
        }

        #[test]
        fn test_parse_mod_cfg_test_filtered() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "#[cfg(test)]\nmod tests;");

            let decls = parse_mod_declarations(&path).unwrap();
            assert!(decls.is_empty());
        }

        #[test]
        fn test_parse_mod_multiple() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "mod alpha;\nmod beta;\nmod gamma;");

            let decls = parse_mod_declarations(&path).unwrap();
            let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
            assert_eq!(names, vec!["alpha", "beta", "gamma"]);
        }

        #[test]
        fn test_parse_mod_inline_ignored() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "mod foo { fn bar() {} }");

            let decls = parse_mod_declarations(&path).unwrap();
            assert!(decls.is_empty());
        }

        #[test]
        fn test_parse_mod_with_path_attribute() {
            let tmp = TempDir::new().unwrap();
            let path = write_rust_file(&tmp, "#[path = \"custom.rs\"]\nmod foo;");

            let decls = parse_mod_declarations(&path).unwrap();
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
}
