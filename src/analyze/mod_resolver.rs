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
    pub(crate) cfg_attr_paths: Vec<CfgAttrPath>,
    /// Where the declaration loads the file it names without a `#[cfg_attr]`
    /// path: where none of the paths holds. `None` where only test builds
    /// load it and tests are left out.
    pub(crate) plain_file: Option<CfgCondition>,
    /// Where the `#[cfg]` of one of the module's declarations holds. A
    /// condition that holds in test builds only makes the module and all it
    /// holds test code.
    pub(crate) condition: CfgCondition,
}

/// A path `#[cfg_attr(predicate, path = …)]` names.
#[derive(PartialEq)]
pub(crate) struct CfgAttrPath {
    pub(crate) path: String,
    /// Where the predicates on the way to the path hold, together as under
    /// `all(…)`.
    pub(crate) condition: CfgCondition,
}

/// Where a cfg condition holds, evaluated by [`evaluate_cfg`] once in the
/// builds without `test` and once in the test builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CfgCondition {
    without_test: Option<bool>,
    test: Option<bool>,
}

impl CfgCondition {
    /// Combine the predicates as `all(…)` does.
    fn all(predicates: &[syn::Meta]) -> Self {
        let evaluate = |test| {
            all_of(
                predicates
                    .iter()
                    .map(|predicate| evaluate_cfg(predicate, test)),
            )
        };
        CfgCondition {
            without_test: evaluate(false),
            test: evaluate(true),
        }
    }

    /// Combine the `#[cfg]`s among `attrs` as `all(…)` does, since an item
    /// needs each to hold. A `#[cfg]` that does not parse is open in every
    /// build.
    pub(crate) fn of_attrs(attrs: &[syn::Attribute]) -> Self {
        let evaluate = |test| {
            all_of(
                attrs
                    .iter()
                    .filter(|attr| attr.path().is_ident("cfg"))
                    .map(|attr| {
                        attr.parse_args::<syn::Meta>()
                            .ok()
                            .and_then(|predicate| evaluate_cfg(&predicate, test))
                    }),
            )
        };
        CfgCondition {
            without_test: evaluate(false),
            test: evaluate(true),
        }
    }

    /// Combine the conditions as `any(…)` does, as for alternatives that
    /// load the same file.
    fn any(conditions: impl IntoIterator<Item = CfgCondition>) -> Self {
        let (without_test, test): (Vec<_>, Vec<_>) = conditions
            .into_iter()
            .map(|condition| (condition.without_test, condition.test))
            .unzip();
        CfgCondition {
            without_test: any_of(without_test),
            test: any_of(test),
        }
    }

    /// Evaluate the condition in the test builds where `test` is set, in the
    /// builds without `test` otherwise, as [`evaluate_cfg`] answers.
    pub(crate) fn in_builds(self, test: bool) -> Option<bool> {
        if test { self.test } else { self.without_test }
    }

    /// Check whether the condition admits test builds only: it holds in no
    /// build without `test`.
    pub(crate) fn test_builds_only(self) -> bool {
        self.without_test == Some(false)
    }

    /// Check whether the condition may hold in a test build: unless it holds
    /// in none.
    pub(crate) fn may_hold_in_test_build(self) -> bool {
        self.test != Some(false)
    }
}

impl std::ops::Not for CfgCondition {
    type Output = CfgCondition;

    fn not(self) -> CfgCondition {
        CfgCondition {
            without_test: self.without_test.map(|holds| !holds),
            test: self.test.map(|holds| !holds),
        }
    }
}

/// Evaluate a cfg predicate in the builds where the atom `test` is as `test`
/// says: `Some(true)` where it holds in every one, `Some(false)` where it
/// holds in none, `None` where the answer depends on another atom. Only
/// `test` is known; `unix`, a feature and every other atom are open.
fn evaluate_cfg(predicate: &syn::Meta, test: bool) -> Option<bool> {
    use syn::parse::Parser;
    let values = |list: &syn::MetaList| {
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .ok()
            .map(|predicates| {
                predicates
                    .iter()
                    .map(|predicate| evaluate_cfg(predicate, test))
                    .collect::<Vec<_>>()
            })
    };
    match predicate {
        syn::Meta::Path(path) if path.is_ident("test") => Some(test),
        syn::Meta::List(list) if list.path.is_ident("not") => list
            .parse_args::<syn::Meta>()
            .ok()
            .and_then(|predicate| evaluate_cfg(&predicate, test))
            .map(|holds| !holds),
        syn::Meta::List(list) if list.path.is_ident("all") => all_of(values(list)?),
        syn::Meta::List(list) if list.path.is_ident("any") => any_of(values(list)?),
        _ => None,
    }
}

/// Combine values of [`evaluate_cfg`] as `all(…)` does: false where one is
/// false, true where each is true, open otherwise.
fn all_of(values: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    values
        .into_iter()
        .try_fold(Some(true), |all, value| match value {
            Some(false) => Err(()),
            Some(true) => Ok(all),
            None => Ok(None),
        })
        .unwrap_or(Some(false))
}

/// Combine values of [`evaluate_cfg`] as `any(…)` does: true where one is
/// true, false where each is false, open otherwise.
fn any_of(values: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    all_of(values.into_iter().map(|value| value.map(|holds| !holds))).map(|holds| !holds)
}

/// Check whether a `#[cfg]` admits the item into test builds only.
pub(crate) fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    CfgCondition::of_attrs(attrs).test_builds_only()
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

/// Extract the values of `path = "..."` inside `#[cfg_attr(predicate, …)]`,
/// also where a `cfg_attr` nests in another. A nested path holds where every
/// predicate on the way to it holds, as under `all(…)`.
fn extract_cfg_attr_paths(attrs: &[syn::Attribute]) -> Vec<CfgAttrPath> {
    let mut paths = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("cfg_attr")) {
        if let Ok(metas) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            collect_cfg_attr_paths(Vec::new(), metas, &mut paths);
        }
    }
    paths
}

/// Collect the paths in the arguments of one `cfg_attr`, its predicate first,
/// under the predicates of the `cfg_attr`s it nests in.
fn collect_cfg_attr_paths(
    mut predicates: Vec<syn::Meta>,
    metas: syn::punctuated::Punctuated<syn::Meta, syn::Token![,]>,
    paths: &mut Vec<CfgAttrPath>,
) {
    use syn::parse::Parser;
    let mut metas = metas.into_iter();
    let Some(predicate) = metas.next() else {
        return;
    };
    predicates.push(predicate);
    for meta in metas {
        match meta {
            syn::Meta::NameValue(syn::MetaNameValue {
                path,
                value:
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }),
                ..
            }) if path.is_ident("path") => {
                paths.push(CfgAttrPath::under(s.value(), &predicates));
            }
            syn::Meta::List(list) if list.path.is_ident("cfg_attr") => {
                if let Ok(nested) =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                        .parse2(list.tokens)
                {
                    collect_cfg_attr_paths(predicates.clone(), nested, paths);
                }
            }
            _ => {}
        }
    }
}

impl CfgAttrPath {
    /// Take `path` as holding where all `predicates` on the way to it hold.
    fn under(path: String, predicates: &[syn::Meta]) -> Self {
        CfgAttrPath {
            path,
            condition: CfgCondition::all(predicates),
        }
    }
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
            let condition = CfgCondition::of_attrs(&item_mod.attrs);
            // Skip #[cfg(test)] modules unless --include-tests was passed
            if !include_tests && condition.test_builds_only() {
                continue;
            }
            let name = item_mod.ident.to_string();
            let explicit_path = extract_path_attribute(&item_mod.attrs);
            let mut cfg_attr_paths = extract_cfg_attr_paths(&item_mod.attrs);
            if !include_tests {
                cfg_attr_paths.retain(|path| !path.condition.test_builds_only());
            }
            let plain = !CfgCondition::any(cfg_attr_paths.iter().map(|path| path.condition));
            let plain_file = (include_tests || !plain.test_builds_only()).then_some(plain);
            // `mod foo;` repeated under complementary cfgs names one file
            if let Some(known) = decls.iter_mut().find(|d| {
                d.name == name
                    && d.explicit_path == explicit_path
                    && d.cfg_attr_paths == cfg_attr_paths
            }) {
                known.condition = CfgCondition::any([known.condition, condition]);
                continue;
            }
            decls.push(ModDecl {
                name,
                explicit_path,
                cfg_attr_paths,
                plain_file,
                condition,
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

/// A file a `mod` declaration loads.
pub(crate) struct DeclaredFile {
    pub(crate) file: ModuleFile,
    /// Where the declaration loads the file: the file without a path where
    /// none of the `#[cfg_attr]` paths holds, a path's file where its
    /// predicates hold, either where both name it. A condition that holds in
    /// test builds only makes the file and all it holds test code.
    pub(crate) condition: CfgCondition,
}

/// The files a `mod` declaration in `file_path` loads. A `#[path]`, plain or
/// in `#[cfg_attr]`, is relative to the directory of `file_path`, and the
/// file it names owns its directory. Each `#[cfg_attr(…, path = …)]` file is
/// an alternative to the file the declaration loads without it; a file that
/// does not exist, or that only test builds load while tests are left out,
/// is left out.
pub(crate) fn declared_files(
    file_path: &Path,
    owns_directory: bool,
    decl: &ModDecl,
) -> Vec<DeclaredFile> {
    let dir = file_path.parent().unwrap_or(Path::new("."));
    let by_path = |path: &String, condition: CfgCondition| {
        let path = dir.join(path);
        path.exists().then_some(DeclaredFile {
            file: ModuleFile {
                path,
                owns_directory: true,
            },
            condition,
        })
    };
    let plain = decl
        .plain_file
        .and_then(|condition| match &decl.explicit_path {
            Some(path) => by_path(path, condition),
            None => resolve_mod_path(&child_resolve_dir(file_path, owns_directory), &decl.name)
                .map(|path| DeclaredFile {
                    file: ModuleFile {
                        path,
                        owns_directory: false,
                    },
                    condition,
                }),
        });
    let mut files: Vec<DeclaredFile> = plain.into_iter().collect();
    for file in decl
        .cfg_attr_paths
        .iter()
        .filter_map(|cfg_attr| by_path(&cfg_attr.path, cfg_attr.condition))
    {
        match files
            .iter_mut()
            .find(|known| known.file.path == file.file.path)
        {
            Some(known) => known.condition = CfgCondition::any([known.condition, file.condition]),
            None => files.push(file),
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

        #[test]
        fn test_is_cfg_test_where_a_double_negation_admits_test_builds_only() {
            let attrs = parse_attrs("#[cfg(not(not(test)))] mod x;");
            assert!(is_cfg_test(&attrs));
            assert!(CfgCondition::of_attrs(&attrs).may_hold_in_test_build());
        }

        #[test]
        fn test_condition_holds_in_no_test_build() {
            let cases = [
                ("#[cfg(test)] mod x;", false),
                ("#[cfg(not(test))] mod x;", true),
                ("#[cfg(any(unix, test))] mod x;", false),
                ("#[cfg(all(unix, test))] mod x;", false),
                ("#[cfg(not(any(unix, test)))] mod x;", true),
                ("#[cfg(all(unix, not(test)))] mod x;", true),
                ("#[cfg(unix)] mod x;", false),
                ("#[cfg(unix)] #[cfg(not(test))] mod x;", true),
                ("mod x;", false),
            ];
            for (code, expected) in cases {
                assert_eq!(
                    !CfgCondition::of_attrs(&parse_attrs(code)).may_hold_in_test_build(),
                    expected,
                    "{code}"
                );
            }
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
                .map(|d| (d.name.as_str(), d.condition.test_builds_only()))
                .collect();
            assert_eq!(cfg_test, [("both", false), ("only", true)]);
        }

        #[test]
        fn test_evaluate_cfg_evaluates_test_and_leaves_other_atoms_open() {
            // (predicate, in test builds, in builds without test)
            let cases = [
                ("test", Some(true), Some(false)),
                ("not(test)", Some(false), Some(true)),
                ("any(unix, test)", Some(true), None),
                ("all(unix, test)", None, Some(false)),
                ("not(any(unix, test))", Some(false), None),
                ("not(not(test))", Some(true), Some(false)),
                ("unix", None, None),
            ];
            for (predicate, test, without_test) in cases {
                let meta: syn::Meta = syn::parse_str(predicate).unwrap();
                assert_eq!(
                    (evaluate_cfg(&meta, true), evaluate_cfg(&meta, false)),
                    (test, without_test),
                    "{predicate}"
                );
            }
        }

        #[test]
        fn test_declaration_test_builds_load_unless_its_cfg_keeps_them_out() {
            let syntax: syn::File = syn::parse_str(
                "#[cfg(not(test))] mod prod; #[cfg(test)] mod both; #[cfg(not(test))] mod both;
                 #[cfg(test)] mod only; mod plain; #[cfg(all(unix, not(test)))] mod unix_prod;",
            )
            .unwrap();
            let loaded: Vec<_> = extract_mod_declarations(&syntax, true)
                .into_iter()
                .map(|d| (d.name, d.condition.may_hold_in_test_build()))
                .collect();
            let owned = |name: &str, loaded| (name.to_string(), loaded);
            assert_eq!(
                loaded,
                [
                    owned("prod", false),
                    owned("both", true),
                    owned("only", true),
                    owned("plain", true),
                    owned("unix_prod", false)
                ]
            );
        }

        #[test]
        fn test_files_test_builds_load_follow_the_cfg_attr_paths() {
            let tmp = TestProject::new()
                .file("src/lib.rs", "")
                .file("src/t.rs", "")
                .file("src/p.rs", "")
                .file("src/u.rs", "")
                .file("src/under_test.rs", "")
                .file("src/outside_test.rs", "")
                .file("src/on_unix.rs", "")
                .build();
            let source = r#"#[cfg_attr(test, path = "t.rs")] mod under_test;
                #[cfg_attr(not(test), path = "p.rs")] mod outside_test;
                #[cfg_attr(all(unix, test), path = "u.rs")] mod on_unix;"#;
            let syntax: syn::File = syn::parse_str(source).unwrap();
            let root = tmp.path().join("src/lib.rs");
            let loaded: Vec<Vec<(String, bool)>> = extract_mod_declarations(&syntax, true)
                .iter()
                .map(|decl| {
                    declared_files(&root, true, decl)
                        .into_iter()
                        .map(|declared| {
                            let name = declared.file.path.file_name().unwrap();
                            (
                                name.to_string_lossy().into_owned(),
                                declared.condition.may_hold_in_test_build(),
                            )
                        })
                        .collect()
                })
                .collect();
            let owned = |name: &str, loaded| (name.to_string(), loaded);
            assert_eq!(
                loaded,
                [
                    vec![owned("under_test.rs", false), owned("t.rs", true)],
                    vec![owned("outside_test.rs", true), owned("p.rs", false)],
                    vec![owned("on_unix.rs", true), owned("u.rs", true)],
                ]
            );
        }

        #[test]
        fn test_cfg_attr_paths_are_read_and_other_cfg_attrs_ignored() {
            let syntax: syn::File = syn::parse_str(
                r#"#[cfg_attr(unix, path = "u.rs")] #[cfg_attr(windows, allow(dead_code), path = "w.rs")]
                   #[cfg_attr(test, allow(dead_code), cfg_attr(unix, path = "n.rs"))] mod imp;
                   #[cfg_attr(test, allow(dead_code))] #[cfg_attr(test, cfg_attr(unix, allow(dead_code)))] mod plain;"#,
            )
            .unwrap();
            let decls = extract_mod_declarations(&syntax, true);
            let paths: Vec<_> = decls
                .iter()
                .map(|d| {
                    let paths: Vec<_> = d.cfg_attr_paths.iter().map(|p| p.path.clone()).collect();
                    (d.name.as_str(), paths)
                })
                .collect();
            assert_eq!(
                paths,
                [
                    (
                        "imp",
                        vec!["u.rs".to_string(), "w.rs".to_string(), "n.rs".to_string()]
                    ),
                    ("plain", vec![])
                ]
            );
        }

        #[test]
        fn test_cfg_attr_path_under_test_predicate_is_test_only() {
            let source = r#"#[cfg_attr(test, path = "t.rs")] #[cfg_attr(all(unix, test), path = "a.rs")]
                #[cfg_attr(any(unix, test), path = "o.rs")]
                #[cfg_attr(test, cfg_attr(unix, path = "nt.rs"))] #[cfg_attr(unix, cfg_attr(test, path = "nu.rs"))]
                #[cfg_attr(unix, cfg_attr(windows, cfg_attr(test, path = "n3.rs")))]
                #[cfg_attr(any(unix, test), cfg_attr(unix, path = "no.rs"))] mod imp;"#;
            let syntax: syn::File = syn::parse_str(source).unwrap();
            let paths = |include_tests| {
                extract_mod_declarations(&syntax, include_tests)[0]
                    .cfg_attr_paths
                    .iter()
                    .map(|p| (p.path.clone(), p.condition.test_builds_only()))
                    .collect::<Vec<_>>()
            };
            let owned = |p: &str, cfg_test| (p.to_string(), cfg_test);
            assert_eq!(
                paths(true),
                [
                    owned("t.rs", true),
                    owned("a.rs", true),
                    owned("o.rs", false),
                    owned("nt.rs", true),
                    owned("nu.rs", true),
                    owned("n3.rs", true),
                    owned("no.rs", false)
                ]
            );
            assert_eq!(paths(false), [owned("o.rs", false), owned("no.rs", false)]);
        }

        #[test]
        fn test_plain_file_is_test_code_where_a_cfg_attr_path_holds_outside_test() {
            let source = r#"#[cfg_attr(not(test), path = "p.rs")] mod not_test;
                #[cfg_attr(all(not(test), not(all(unix, test))), path = "p.rs")] mod all_outside;
                #[cfg_attr(any(unix, not(test)), path = "p.rs")] mod any_outside;
                #[cfg_attr(all(unix, not(test)), path = "p.rs")] mod all_unix;
                #[cfg_attr(not(unix), path = "p.rs")] mod not_unix;
                #[cfg_attr(not(any(unix, test)), path = "p.rs")] mod not_any;
                #[cfg_attr(not(test), allow(dead_code))] mod no_path;
                #[cfg_attr(not(test), cfg_attr(not(test), path = "p.rs"))] mod nested_outside;
                #[cfg_attr(not(test), cfg_attr(unix, path = "p.rs"))] mod nested_unix;
                #[cfg_attr(unix, cfg_attr(not(test), path = "p.rs"))] mod nested_unix_outer;
                #[cfg_attr(not(not(not(test))), path = "p.rs")] mod double_negation;"#;
            let syntax: syn::File = syn::parse_str(source).unwrap();
            let plain = |include_tests| {
                extract_mod_declarations(&syntax, include_tests)
                    .into_iter()
                    .map(|d| (d.name, d.plain_file.map(CfgCondition::test_builds_only)))
                    .collect::<Vec<_>>()
            };
            // Some build without `test` loads the plain file.
            let production = Some(false);
            let owned = |name: &str, plain_file| (name.to_string(), plain_file);
            let expected = |test_only| {
                vec![
                    owned("not_test", test_only),
                    owned("all_outside", test_only),
                    owned("any_outside", test_only),
                    owned("all_unix", production),
                    owned("not_unix", production),
                    owned("not_any", production),
                    owned("no_path", production),
                    owned("nested_outside", test_only),
                    owned("nested_unix", production),
                    owned("nested_unix_outer", production),
                    owned("double_negation", test_only),
                ]
            };
            assert_eq!(plain(true), expected(Some(true)));
            assert_eq!(plain(false), expected(None));
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
