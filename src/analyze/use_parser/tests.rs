use super::*;
use crate::analyze::packages::normalize_crate_name;
use crate::analyze::test_workspace::{Analyzed, TestWorkspace};
use crate::model::{DefKind, Definition, EdgeContext, SymbolUse, UseCategory};
use std::path::Path;

/// Define the public item `name` of `kind` in `info`.
fn define(info: &mut ModuleExportInfo, name: &str, kind: DefKind) {
    define_with(info, name, kind, Visibility::Public);
}

/// Define the item `name` of `kind` with `visibility` in `info`.
fn define_with(info: &mut ModuleExportInfo, name: &str, kind: DefKind, visibility: Visibility) {
    let definition = BindingKind::Definition(Definition { kind, line: 1 });
    info.bind(name.to_string(), NameBinding::new(definition, visibility));
}

mod reexport_visibility_tests {
    use super::*;

    fn parse_visibility(code: &str) -> syn::Visibility {
        let file: syn::File = syn::parse_str(code).unwrap();
        match &file.items[0] {
            syn::Item::Use(u) => u.vis.clone(),
            syn::Item::Struct(s) => s.vis.clone(),
            _ => panic!("expected use or struct item"),
        }
    }

    #[test]
    fn pub_is_reexport() {
        let vis = parse_visibility("pub use foo::Bar;");
        assert!(is_reexport_visibility(&vis));
    }

    #[test]
    fn pub_crate_is_reexport() {
        let vis = parse_visibility("pub(crate) use foo::Bar;");
        assert!(is_reexport_visibility(&vis));
    }

    #[test]
    fn pub_super_is_reexport() {
        let vis = parse_visibility("pub(super) use foo::Bar;");
        assert!(is_reexport_visibility(&vis));
    }

    #[test]
    fn private_is_not_reexport() {
        let vis = parse_visibility("use foo::Bar;");
        assert!(!is_reexport_visibility(&vis));
    }

    #[test]
    fn pub_in_path_is_not_reexport() {
        let vis = parse_visibility("pub(in crate::parent) use foo::Bar;");
        assert!(!is_reexport_visibility(&vis));
    }
}

mod normalize_tests {
    use super::*;

    #[test]
    fn test_normalize_crate_name() {
        assert_eq!(normalize_crate_name("my-lib"), "my_lib");
        assert_eq!(normalize_crate_name("already_valid"), "already_valid");
        assert_eq!(normalize_crate_name("a-b-c"), "a_b_c");
    }

    #[test]
    fn test_process_use_statement_crate_local() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod cli;\nmod graph;"),
                    ("src/cli.rs", "use crate::graph::build;"),
                    ("src/graph.rs", "pub fn build() {}"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::cli"),
            [("my_crate", "graph", Some("build"))]
        );
    }

    #[test]
    fn test_process_use_statement_crate_local_module_only() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod cli;\nmod graph;"),
                    ("src/cli.rs", "use crate::graph;"),
                    ("src/graph.rs", ""),
                ],
            )
            .analyze();
        assert_eq!(ws.targets("my_crate::cli"), [("my_crate", "graph", None)]);
        assert_eq!(ws.deps("my_crate::cli")[0].line, 1);
    }

    #[test]
    fn test_process_use_statement_workspace_crate() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[("src/lib.rs", "pub mod utils;"), ("src/utils.rs", "")],
            )
            .package("my_crate", &[("src/lib.rs", "use other_crate::utils;")])
            .analyze();
        assert_eq!(ws.targets("my_crate"), [("other_crate", "utils", None)]);
    }

    #[test]
    fn test_process_use_statement_workspace_crate_with_hyphen() {
        let ws = TestWorkspace::new()
            .package(
                "my-lib",
                &[("src/lib.rs", "pub mod feature;"), ("src/feature.rs", "")],
            )
            .package(
                "app",
                &[("src/main.rs", "use my_lib::feature;\nfn main() {}")],
            )
            .analyze();
        assert_eq!(ws.targets("app"), [("my_lib", "feature", None)]);
    }

    #[test]
    fn test_process_use_statement_relative_self_resolved() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod render;"),
                    ("src/render/mod.rs", "mod helper;\nuse self::helper;"),
                    ("src/render/helper.rs", ""),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::render"),
            [("my_crate", "render::helper", None)]
        );
    }

    #[test]
    fn test_process_use_statement_relative_super_resolved() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod parent;\nmod sub;"),
                    ("src/sub/mod.rs", "use super::parent;"),
                    ("src/parent.rs", ""),
                ],
            )
            .analyze();
        assert_eq!(ws.targets("my_crate::sub"), [("my_crate", "parent", None)]);
    }

    #[test]
    fn test_process_use_statement_external_filtered() {
        let ws = TestWorkspace::new()
            .package("my_crate", &[("src/lib.rs", "use serde::Serialize;")])
            .analyze();
        assert!(
            ws.deps("my_crate").is_empty(),
            "external crate imports should be filtered"
        );
    }

    #[test]
    fn test_process_use_statement_std_filtered() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[("src/lib.rs", "use std::collections::HashMap;")],
            )
            .analyze();
        assert!(
            ws.deps("my_crate").is_empty(),
            "std imports should be filtered"
        );
    }
}

mod submodule_tests {
    use super::*;

    #[test]
    fn test_crate_local_submodule() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod analyze;\nmod cli;"),
                    ("src/analyze/mod.rs", "pub mod use_parser;"),
                    ("src/analyze/use_parser.rs", "pub fn normalize() {}"),
                    ("src/cli.rs", "use crate::analyze::use_parser::normalize;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::cli"),
            [("my_crate", "analyze::use_parser", Some("normalize"))]
        );
    }

    #[test]
    fn test_workspace_import_submodule() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod foo;"),
                    ("src/foo/mod.rs", "pub mod bar;"),
                    ("src/foo/bar.rs", "pub struct Baz;"),
                ],
            )
            .package(
                "my_crate",
                &[("src/lib.rs", "use other_crate::foo::bar::Baz;")],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "foo::bar", Some("Baz"))]
        );
    }

    #[test]
    fn a_descendant_takes_the_item_of_a_path_written_through_a_written_path() {
        let ws = TestWorkspace::new()
            .package(
                "base",
                &[
                    ("src/lib.rs", "pub mod deep;\npub use deep::outer;"),
                    ("src/deep.rs", "pub mod outer;"),
                    ("src/deep/outer.rs", "pub mod inner;"),
                    ("src/deep/outer/inner.rs", "pub struct Thing;"),
                ],
            )
            .package("middle", &[("src/lib.rs", "pub use base::outer::inner;")])
            .package(
                "top",
                &[
                    ("src/lib.rs", "use middle::inner::Thing;\nmod user;"),
                    ("src/user.rs", "use crate::Thing;\npub fn f(_: Thing) {}"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("top::user"),
            [("base", "deep::outer::inner", Some("Thing"))]
        );
    }

    #[test]
    fn test_workspace_import_through_a_module_another_module_reexports() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod a;\npub mod store;"),
                    ("src/a.rs", "pub use crate::store as bound;"),
                    ("src/store.rs", "pub struct Set;"),
                ],
            )
            .package(
                "my_crate",
                &[("src/lib.rs", "use other_crate::a::bound::Set;")],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "store", Some("Set"))]
        );
    }

    #[test]
    fn a_glob_of_variants_of_another_crate_brings_no_name_of_its_root() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[("src/lib.rs", "pub enum Mode { Fast }\npub struct Other;")],
            )
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod holder;"),
                    ("src/holder.rs", "use other_crate::Mode::*;\nmod child;"),
                    ("src/holder/child.rs", "use super::Other;"),
                ],
            )
            .analyze();
        let child_targets = ws.targets("my_crate::holder::child");
        assert!(
            !child_targets.contains(&("other_crate", "", Some("Other"))),
            "{child_targets:?}"
        );
    }

    #[test]
    fn test_workspace_import_through_root_glob_reexport() {
        // The root file names `Type` nowhere, only the glob brings it in.
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "mod ty;\npub use ty::*;"),
                    ("src/ty.rs", "pub enum Type {}"),
                ],
            )
            .package("my_crate", &[("src/lib.rs", "use other_crate::Type;")])
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "ty", Some("Type"))]
        );
    }

    #[test]
    fn test_workspace_import_through_module_from_root_glob() {
        // The glob brings the module `shell_error` into the root, and the path
        // continues through it.
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "mod errors;\npub use errors::*;"),
                    ("src/errors/mod.rs", "pub mod shell_error;"),
                    ("src/errors/shell_error.rs", "pub enum ShellError {}"),
                ],
            )
            .package(
                "my_crate",
                &[("src/lib.rs", "use other_crate::shell_error::ShellError;")],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "errors::shell_error", Some("ShellError"))]
        );
    }

    #[test]
    fn test_workspace_import_through_module_from_named_root_reexport() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod backend;\nmod dispatch;\npub use backend::custom;",
                    ),
                    ("src/backend/mod.rs", "pub mod custom;"),
                    (
                        "src/backend/custom.rs",
                        "pub use crate::dispatch::Interface;",
                    ),
                    ("src/dispatch.rs", "pub trait Interface {}"),
                ],
            )
            .package(
                "my_crate",
                &[("src/lib.rs", "use other_crate::custom::Interface;")],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "dispatch", Some("Interface"))]
        );
    }

    #[test]
    fn test_workspace_import_through_extern_crate_alias() {
        // `oc` names other_crate in every module of the crate.
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod dispatch;"),
                    ("src/dispatch.rs", "pub trait Interface {}"),
                ],
            )
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "extern crate other_crate as oc;\nmod user;"),
                    ("src/user.rs", "use oc::dispatch::Interface;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("other_crate", "dispatch", Some("Interface"))]
        );
    }

    #[test]
    fn test_crate_local_import_through_module_from_root_glob() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "pub mod frame;\nmod user;\npub use frame::*;"),
                    ("src/frame/mod.rs", "pub mod values;"),
                    ("src/frame/values.rs", "pub struct Value;"),
                    ("src/user.rs", "use crate::values::Value;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "frame::values", Some("Value"))]
        );
    }

    #[test]
    fn test_crate_local_import_through_root_glob_reexport() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod ty;\nmod user;\npub use ty::*;"),
                    ("src/ty.rs", "pub enum Type {}"),
                    ("src/user.rs", "use crate::Type;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "ty", Some("Type"))]
        );
    }

    #[test]
    fn test_crate_local_import_through_named_root_reexport() {
        // `eval_call` is a fn, not a module.
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod eval;\nmod user;\npub use eval::{eval_call};",
                    ),
                    ("src/eval.rs", "pub fn eval_call() {}"),
                    ("src/user.rs", "use crate::eval_call;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "eval", Some("eval_call"))]
        );
    }

    #[test]
    fn test_crate_local_import_through_a_reexported_crate() {
        let ws = TestWorkspace::new()
            .package(
                "leaf_crate",
                &[
                    ("src/lib.rs", "pub mod error;"),
                    ("src/error.rs", "pub struct ParseError;"),
                ],
            )
            .package(
                "mid_crate",
                &[("src/lib.rs", "pub use leaf_crate as leaf;")],
            )
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod user;\npub use mid_crate::leaf;"),
                    ("src/user.rs", "use crate::leaf::error::ParseError;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("leaf_crate", "error", Some("ParseError"))]
        );
    }

    #[test]
    fn test_workspace_import_through_a_crate_another_root_reexports() {
        let ws = TestWorkspace::new()
            .package(
                "leaf_crate",
                &[
                    ("src/lib.rs", "pub mod error;"),
                    ("src/error.rs", "pub struct ParseError;"),
                ],
            )
            .package(
                "mid_crate",
                &[
                    ("src/lib.rs", "pub mod device;\npub use leaf_crate;"),
                    ("src/device.rs", ""),
                ],
            )
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod user;"),
                    (
                        "src/user.rs",
                        "use mid_crate::leaf_crate::error::ParseError;",
                    ),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("leaf_crate", "error", Some("ParseError"))]
        );
    }

    #[test]
    fn test_crate_local_import_of_item_defined_in_root() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod user;\npub struct Server;"),
                    ("src/user.rs", "use crate::Server;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "", Some("Server"))]
        );
    }

    #[test]
    fn test_crate_local_import_of_private_item_in_root() {
        // A private item in the root is visible to every module of the crate.
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod user;\nfn helper() {}"),
                    ("src/user.rs", "use crate::helper;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "", Some("helper"))]
        );
    }

    /// A root that defines the enum `Kind` with the variant `One` through a
    /// macro, so that no binding of the root names `Kind`.
    const ROOT_WITH_A_MACRO_ENUM: &str = r"
macro_rules! make_enum {
    ($name:ident) => {
        pub enum $name {
            One,
        }
    };
}
make_enum!(Kind);
";

    #[test]
    fn test_cross_crate_path_through_an_item_a_macro_defines_ends_on_it() {
        // other_crate binds no `Kind`: the name is an item of its root.
        let ws = TestWorkspace::new()
            .package("other_crate", &[("src/lib.rs", ROOT_WITH_A_MACRO_ENUM)])
            .package("my_crate", &[("src/lib.rs", "use other_crate::Kind::One;")])
            .analyze();
        assert_eq!(ws.targets("my_crate"), [("other_crate", "", Some("Kind"))]);
    }

    #[test]
    fn test_crate_local_path_through_an_item_a_macro_defines_ends_on_it() {
        let root = format!("mod analyze;\nmod cli;\n{ROOT_WITH_A_MACRO_ENUM}");
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", &root),
                    ("src/analyze.rs", ""),
                    ("src/cli.rs", "use crate::Kind::One;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::cli"),
            [("my_crate", "", Some("Kind"))]
        );
    }

    #[test]
    fn test_multi_symbol_with_submodule() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod analyze;\nmod cli;"),
                    ("src/analyze/mod.rs", "pub mod use_parser;"),
                    (
                        "src/analyze/use_parser.rs",
                        "pub fn normalize() {}\npub fn is_workspace_member() {}",
                    ),
                    (
                        "src/cli.rs",
                        "use crate::analyze::use_parser::{normalize, is_workspace_member};",
                    ),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate::cli"),
            [
                (
                    "my_crate",
                    "analyze::use_parser",
                    Some("is_workspace_member")
                ),
                ("my_crate", "analyze::use_parser", Some("normalize")),
            ]
        );
    }
}

mod parsing_tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn test_parse_workspace_dependencies_mixed() {
        let source = "mod graph;
use crate::graph;
use other_crate::utils;
use serde::Serialize;
use std::collections::HashMap;
";
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[("src/lib.rs", "pub mod utils;"), ("src/utils.rs", "")],
            )
            .package("my_crate", &[("src/lib.rs", source), ("src/graph.rs", "")])
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate"),
            [("my_crate", "graph", None), ("other_crate", "utils", None)]
        );
    }

    #[test]
    fn test_parse_workspace_dependencies_dedup_by_full_target() {
        let source = "use crate::graph::build;
use crate::graph::Node;
use crate::graph;
";
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod cli;\nmod graph;"),
                    ("src/cli.rs", source),
                    ("src/graph.rs", "pub fn build() {}\npub struct Node;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate::cli"),
            [
                ("my_crate", "graph", None),
                ("my_crate", "graph", Some("Node")),
                ("my_crate", "graph", Some("build")),
            ],
            "should keep distinct symbols"
        );
    }

    #[test]
    fn test_process_use_multi_symbol() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod cli;\nmod graph;"),
                    ("src/cli.rs", "use crate::graph::{Node, Edge};"),
                    ("src/graph.rs", "pub struct Node;\npub struct Edge;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate::cli"),
            [
                ("my_crate", "graph", Some("Edge")),
                ("my_crate", "graph", Some("Node")),
            ]
        );
    }

    #[test]
    fn test_process_use_glob() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod analyze;\nmod cli;"),
                    ("src/analyze.rs", "pub struct Walker;"),
                    ("src/cli.rs", "use crate::analyze::*;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::cli"),
            [("my_crate", "analyze", Some("*"))]
        );
    }

    /// Analyze `cli.rs` written as `source` beside a module `analyze` that
    /// defines `Walker` and `analyze_module`, and a module `other` that
    /// defines another `Walker`.
    fn glob_deps(source: &str) -> Vec<DependencyRef> {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod analyze;\nmod cli;\nmod other;"),
                    (
                        "src/analyze.rs",
                        "pub struct Walker;\npub fn analyze_module() {}",
                    ),
                    ("src/other.rs", "pub struct Walker;"),
                    ("src/cli.rs", source),
                ],
            )
            .analyze()
            .deps("my_crate::cli")
            .to_vec()
    }

    fn items_of(deps: &[DependencyRef]) -> Vec<&str> {
        deps.iter()
            .filter_map(|d| d.target_item.as_deref())
            .collect()
    }

    /// A glob brings in the names the file goes on to use, each with its
    /// category, at the glob's line. A name it could bring in but the file never
    /// writes is not a symbol on the edge.
    #[test]
    fn test_process_use_glob_expands_to_the_names_used() {
        let deps = glob_deps("use crate::analyze::*;\nfn f(w: Walker) { analyze_module(); }");

        assert_eq!(
            items_of(&deps),
            ["Walker", "analyze_module"],
            "deps: {deps:?}"
        );
        assert!(
            deps.iter()
                .all(|d| d.target_module == "analyze" && d.line == 1)
        );
        assert_eq!(
            deps[0].uses,
            [SymbolUse {
                line: 2,
                category: UseCategory::Types,
                imported_for_methods: false,
            }]
        );
        assert_eq!(deps[1].uses[0].category, UseCategory::Fns);
    }

    /// A `pub use` glob republishes every name and uses none, so it carries
    /// the whole payload; a name the file also writes is a use on that name.
    #[test]
    fn test_process_pub_use_glob_carries_the_whole_payload() {
        let deps = glob_deps("pub use crate::analyze::*;\nfn f(w: Walker) {}");
        assert_eq!(
            items_of(&deps),
            ["Walker", "analyze_module"],
            "deps: {deps:?}"
        );
        assert!(deps.iter().all(|d| d.via_reexport), "deps: {deps:?}");
        assert_eq!(deps[0].uses.len(), 1, "the parameter type: {deps:?}");
    }

    /// A name written only inside macro input still counts as used.
    #[test]
    fn test_process_use_glob_finds_names_in_macro_input() {
        let deps = glob_deps("use crate::analyze::*;\nfn f() { assert!(analyze_module()); }");
        assert_eq!(items_of(&deps), ["analyze_module"], "deps: {deps:?}");
    }

    /// A glob whose names the file never writes keeps the `*`: the import is
    /// there, and the edge with it.
    #[test]
    fn test_process_use_glob_unused_keeps_marker() {
        let deps = glob_deps("use crate::analyze::*;\nfn f() {}");
        assert_eq!(items_of(&deps), ["*"], "deps: {deps:?}");
    }

    /// An explicit `use` and an item of the file itself both take the name
    /// away from the glob.
    #[test]
    fn test_process_use_glob_yields_to_explicit_binding_and_local_definition() {
        let deps = glob_deps(
            "use crate::analyze::*;\nuse crate::other::Walker;\nfn analyze_module() {}\n\
             fn f(w: Walker) { analyze_module(); }",
        );
        assert!(
            !deps
                .iter()
                .any(|d| d.target_module == "analyze" && d.target_item.as_deref() != Some("*")),
            "nothing from the glob is used: {deps:?}"
        );
    }

    /// A local variable hides a name the glob brings in, from where the
    /// variable is declared to the end of its scope.
    #[rstest]
    #[case::let_binding("fn f() { let analyze_module = 1; drop(analyze_module); }")]
    #[case::mutable_let("fn f() { let mut analyze_module = 1; analyze_module += 1; }")]
    #[case::parameter("fn f(analyze_module: u8) { drop(analyze_module); }")]
    #[case::closure_parameter("fn f() { let g = |analyze_module: u8| analyze_module(); }")]
    #[case::field_pattern(
        "fn f(v: V) { let V { val: analyze_module } = v; drop(analyze_module); }"
    )]
    #[case::match_arm(
        "fn f(v: Option<u8>) { match v { Some(analyze_module) => drop(analyze_module), None => {} } }"
    )]
    #[case::if_let(
        "fn f(v: Option<u8>) { if let Some(analyze_module) = v { drop(analyze_module); } }"
    )]
    #[case::for_loop("fn f(v: Vec<u8>) { for analyze_module in v { drop(analyze_module); } }")]
    #[case::macro_input("fn f() { let analyze_module = 1; println!(\"{}\", analyze_module); }")]
    fn test_process_use_glob_yields_to_a_local_variable(#[case] body: &str) {
        let deps = glob_deps(&format!("use crate::analyze::*;\n{body}"));
        assert_eq!(items_of(&deps), ["*"], "deps: {deps:?}");
    }

    /// Outside the variable's scope, and in the expression that initializes
    /// it, the name is still the glob's.
    #[rstest]
    #[case::before_the_let("fn f() { analyze_module(); let analyze_module = 1; }")]
    #[case::in_the_initializer("fn f() { let analyze_module = analyze_module(); }")]
    #[case::after_the_block("fn f() { { let analyze_module = 1; } analyze_module(); }")]
    #[case::in_the_else_branch(
        "fn f(v: Option<u8>) { if let Some(analyze_module) = v {} else { analyze_module(); } }"
    )]
    #[case::in_another_fn("fn f(analyze_module: u8) {}\nfn g() { analyze_module(); }")]
    #[case::in_a_nested_fn("fn f() { let analyze_module = 1; fn g() { analyze_module(); } }")]
    fn test_process_use_glob_name_outside_a_local_scope_is_the_globs(#[case] body: &str) {
        let deps = glob_deps(&format!("use crate::analyze::*;\n{body}"));
        assert_eq!(items_of(&deps), ["analyze_module"], "deps: {deps:?}");
    }

    /// A glob written in a block binds in that block only.
    #[test]
    fn test_process_use_glob_binds_in_its_region() {
        let deps = glob_deps(
            "fn f() { use crate::analyze::*; analyze_module(); }\nfn g() { analyze_module(); }",
        );
        assert_eq!(items_of(&deps), ["analyze_module"], "deps: {deps:?}");
        assert_eq!(deps[0].uses.len(), 1, "the call in `g` is not the glob's");
    }

    /// A glob re-export republishes the names it pulls in, so each expanded name
    /// resolves on to the module that defines it — same as a named `pub use`.
    #[test]
    fn test_process_use_glob_expands_through_reexport() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod cli;\nmod facade;\nmod origin;"),
                    ("src/facade.rs", "pub use crate::origin::Widget;"),
                    ("src/origin.rs", "pub struct Widget;"),
                    ("src/cli.rs", "use crate::facade::*;\nfn f(w: Widget) {}"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::cli"),
            [("my_crate", "origin", Some("Widget"))]
        );
    }

    /// An unknown payload keeps the `*`: one unnamed symbol, not zero. A
    /// macro declares `analyze`, so the module tree does not hold it and the
    /// glob stays on the root.
    #[test]
    fn test_process_use_glob_unknown_payload_keeps_marker() {
        let root = r"
mod cli;
macro_rules! make_mod {
    () => {
        pub mod analyze {
            pub struct Walker;
        }
    };
}
make_mod!();
";
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", root),
                    ("src/cli.rs", "use crate::analyze::*;\nfn f(w: Walker) {}"),
                ],
            )
            .analyze();
        assert_eq!(ws.targets("my_crate::cli"), [("my_crate", "", Some("*"))]);
    }
}

mod integration_tests {
    use super::*;

    #[test]
    fn test_bare_module_pub_use_integration() {
        let source = "mod cli;
mod graph;
mod model;
pub use cli::{Args, Cargo, run};
use crate::graph::build;
pub(crate) use model::Node;
use other_crate::utils;
use serde::Serialize;
";
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[("src/lib.rs", "pub mod utils;"), ("src/utils.rs", "")],
            )
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", source),
                    (
                        "src/cli.rs",
                        "pub struct Args;\npub struct Cargo;\npub fn run() {}",
                    ),
                    ("src/graph.rs", "pub fn build() {}"),
                    ("src/model.rs", "pub struct Node;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate"),
            [
                ("my_crate", "cli", Some("Args")),
                ("my_crate", "cli", Some("Cargo")),
                ("my_crate", "cli", Some("run")),
                ("my_crate", "graph", Some("build")),
                ("my_crate", "model", Some("Node")),
                ("other_crate", "utils", None),
            ]
        );
    }
}

mod bare_module_tests {
    use super::*;

    /// Analyze the crate `my_crate` whose root declares `mod cli;` and then
    /// holds `source`; cli.rs defines `Args`, `Cargo` and `run`.
    fn with_cli(source: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", &format!("mod cli;\n{source}")),
                    (
                        "src/cli.rs",
                        "pub struct Args;\npub struct Cargo;\npub fn run() {}",
                    ),
                ],
            )
            .analyze()
    }

    #[test]
    fn test_bare_module_no_match() {
        let ws = with_cli("use serde::Serialize;");
        assert!(
            ws.deps("my_crate").is_empty(),
            "external crate should not match"
        );
    }

    #[test]
    fn test_bare_module_deep_path() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod analyze;\nuse analyze::use_parser::normalize;",
                    ),
                    ("src/analyze/mod.rs", "pub mod use_parser;"),
                    ("src/analyze/use_parser.rs", "pub fn normalize() {}"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("my_crate", "analyze::use_parser", Some("normalize"))]
        );
    }

    #[test]
    fn test_bare_module_module_only() {
        let ws = with_cli("use cli as command_line;");
        assert_eq!(ws.targets("my_crate"), [("my_crate", "cli", None)]);
    }

    #[test]
    fn test_bare_module_via_parse_workspace_dependencies() {
        let ws = with_cli("use cli::Args;");
        assert_eq!(ws.targets("my_crate"), [("my_crate", "cli", Some("Args"))]);
        assert!(!ws.deps("my_crate")[0].via_reexport);
    }

    #[test]
    fn test_bare_module_pub_use_via_parse_workspace_dependencies() {
        let ws = with_cli("pub(crate) use cli::Args;");
        assert_eq!(ws.targets("my_crate"), [("my_crate", "cli", Some("Args"))]);
        assert!(ws.deps("my_crate")[0].via_reexport);
    }

    #[test]
    fn test_bare_module_multi_import() {
        let ws = with_cli("use cli::{Args, Cargo, run};");
        assert_eq!(
            ws.sorted_targets("my_crate"),
            [
                ("my_crate", "cli", Some("Args")),
                ("my_crate", "cli", Some("Cargo")),
                ("my_crate", "cli", Some("run")),
            ]
        );
    }

    #[test]
    fn test_bare_module_glob_import() {
        let ws = with_cli("use cli::*;");
        assert_eq!(ws.targets("my_crate"), [("my_crate", "cli", Some("*"))]);
    }

    #[test]
    fn test_bare_module_child_resolution() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod render;"),
                    ("src/render/mod.rs", "mod css;\nuse css::render_styles;"),
                    ("src/render/css.rs", "pub fn render_styles() {}"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::render"),
            [("my_crate", "render::css", Some("render_styles"))]
        );
    }

    #[test]
    fn test_bare_module_child_multi_import() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod render;"),
                    (
                        "src/render/mod.rs",
                        "mod elements;\nuse elements::{LinkTag, ScriptTag};",
                    ),
                    (
                        "src/render/elements.rs",
                        "pub struct LinkTag;\npub struct ScriptTag;",
                    ),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate::render"),
            [
                ("my_crate", "render::elements", Some("LinkTag")),
                ("my_crate", "render::elements", Some("ScriptTag")),
            ]
        );
    }

    #[test]
    fn test_bare_module_deeply_nested() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod a;"),
                    ("src/a/mod.rs", "mod b;"),
                    ("src/a/b/mod.rs", "mod sub;\nuse sub::Item;"),
                    ("src/a/b/sub.rs", "pub struct Item;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::a::b"),
            [("my_crate", "a::b::sub", Some("Item"))]
        );
    }

    /// A top-level module is in scope of the crate root only. From a sibling
    /// file its bare name is not the module; without a `use` it can only be an
    /// extern crate, so nothing of this crate resolves.
    #[test]
    fn test_bare_module_root_sibling_not_in_scope() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod config;\nmod error;"),
                    ("src/config.rs", "pub struct ConfigError;"),
                    ("src/error.rs", "use config::ConfigError;"),
                ],
            )
            .analyze();
        assert!(
            ws.deps("my_crate::error").is_empty(),
            "a root sibling is not in scope: {:?}",
            ws.deps("my_crate::error")
        );
    }

    /// The same collision seen through a path: `config::ConfigError` in
    /// `error.rs` beside a module `config` yields no edge to that module.
    #[test]
    fn test_root_sibling_path_ref_is_no_dependency() {
        let source = r"
impl From<config::ConfigError> for ConfigError {
    fn from(e: config::ConfigError) -> Self { ConfigError::Load(e) }
}
";
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod config;\nmod error;"),
                    ("src/config.rs", "pub struct ConfigError;"),
                    ("src/error.rs", source),
                ],
            )
            .analyze();
        let deps = ws.deps("my_crate::error");
        assert!(
            deps.iter().all(|d| d.target_module != "config"),
            "no edge to the root sibling module: {deps:?}"
        );
    }
}

mod resolve_use_tree_tests {
    use super::*;

    fn parse_use_tree(source: &str) -> syn::UseTree {
        let file: syn::File = syn::parse_str(source).unwrap();
        match file.items.into_iter().next().unwrap() {
            syn::Item::Use(u) => u.tree,
            _ => panic!("expected use item"),
        }
    }

    #[test]
    fn test_simple_path() {
        let tree = parse_use_tree("use crate::graph::build;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["crate::graph::build"]);
    }

    #[test]
    fn test_multi_import() {
        let tree = parse_use_tree("use cli::{Args, Cargo};");
        let mut paths = resolve_use_tree(&tree, "", false);
        paths.sort();
        assert_eq!(paths, vec!["cli::Args", "cli::Cargo"]);
    }

    #[test]
    fn test_glob() {
        let tree = parse_use_tree("use model::*;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["model::*"]);
    }

    #[test]
    fn test_rename() {
        let tree = parse_use_tree("use cli::Args as CliArgs;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["cli::Args"]);
    }

    #[test]
    fn test_nested_groups() {
        let tree = parse_use_tree("use a::{b::{C, D}, e::F};");
        let mut paths = resolve_use_tree(&tree, "", false);
        paths.sort();
        assert_eq!(paths, vec!["a::b::C", "a::b::D", "a::e::F"]);
    }

    #[test]
    fn test_empty_prefix_root_level() {
        let tree = parse_use_tree("use std;");
        let paths = resolve_use_tree(&tree, "", false);
        assert_eq!(paths, vec!["std"]);
    }

    #[test]
    fn test_rename_with_alias() {
        let tree = parse_use_tree("use cli::Args as CliArgs;");
        let paths = resolve_use_tree(&tree, "", true);
        assert_eq!(paths, vec!["cli::CliArgs"]);
    }

    #[test]
    fn test_with_prefix() {
        let tree = parse_use_tree("use bar::Baz;");
        let paths = resolve_use_tree(&tree, "foo", false);
        assert_eq!(paths, vec!["foo::bar::Baz"]);
    }
}

mod entry_point_tests {
    use super::*;

    /// Analyze `my_crate` whose root holds `source`, beside `other_crate`
    /// whose root is `other_root`.
    fn importing(other_root: &str, source: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", other_root),
                    ("src/sub_mod.rs", "pub struct Foo;"),
                ],
            )
            .package("my_crate", &[("src/lib.rs", source)])
            .analyze()
    }

    #[test]
    fn test_entry_point_export_detected() {
        let ws = importing(
            "pub mod sub_mod;\npub struct MyStruct;",
            "use other_crate::MyStruct;",
        );
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "", Some("MyStruct"))]
        );
    }

    /// A root that defines the unit structs `Foo` and `Bar` through a macro,
    /// so that no binding of the root names them.
    const ROOT_WITH_MACRO_ITEMS: &str = r"
macro_rules! make {
    ($name:ident) => {
        pub struct $name;
    };
}
make!(Foo);
make!(Bar);
";

    #[test]
    fn test_item_a_macro_defines_is_an_item_of_the_root() {
        let root = format!("pub mod sub_mod;\n{ROOT_WITH_MACRO_ITEMS}");
        let ws = importing(&root, "use other_crate::Foo;");
        assert_eq!(ws.targets("my_crate"), [("other_crate", "", Some("Foo"))]);
    }

    #[test]
    fn test_real_module_not_affected() {
        let ws = importing("pub mod sub_mod;", "use other_crate::sub_mod::Foo;");
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "sub_mod", Some("Foo"))]
        );
    }

    #[test]
    fn test_entry_point_multi_import() {
        // No binding of other_crate names either, so each is an item of its root.
        let ws = importing(ROOT_WITH_MACRO_ITEMS, "use other_crate::{Foo, Bar};");
        assert_eq!(
            ws.sorted_targets("my_crate"),
            [
                ("other_crate", "", Some("Bar")),
                ("other_crate", "", Some("Foo"))
            ]
        );
    }

    #[test]
    fn test_entry_point_glob_import() {
        let ws = importing("", "use other_crate::*;");
        assert_eq!(ws.targets("my_crate"), [("other_crate", "", Some("*"))]);
    }
}

mod path_ref_resolution_tests {
    use super::*;

    /// Analyze `my_crate` whose root is `source`, beside `other_crate` with a
    /// module `module` and the root item `MyStruct`.
    fn referencing(source: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod module;\npub struct MyStruct;"),
                    ("src/module.rs", "pub fn item() {}"),
                ],
            )
            .package("my_crate", &[("src/lib.rs", source)])
            .analyze()
    }

    #[test]
    fn test_parse_path_refs_workspace_crate() {
        let ws = referencing("fn main() {\n\n\n\n    other_crate::module::item();\n}");
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "module", Some("item"))]
        );
        assert_eq!(ws.deps("my_crate")[0].line, 5);
    }

    #[test]
    fn test_parse_path_refs_crate_local() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod module;\nfn main() { crate::module::item(); }",
                    ),
                    ("src/module.rs", "pub fn item() {}"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("my_crate", "module", Some("item"))]
        );
    }

    #[test]
    fn test_parse_path_refs_bare_module() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod cli;\nfn run(_: cli::Args) {}"),
                    ("src/cli.rs", "pub struct Args;"),
                ],
            )
            .analyze();
        assert_eq!(ws.targets("my_crate"), [("my_crate", "cli", Some("Args"))]);
    }

    #[test]
    fn test_parse_path_refs_unknown_skipped() {
        let ws = referencing(
            "fn f(_: &dyn std::io::Read) -> anyhow::Result<()> { serde::Serialize::x() }",
        );
        assert!(
            ws.deps("my_crate").is_empty(),
            "unknown paths should be skipped: {:?}",
            ws.deps("my_crate")
        );
    }

    #[test]
    fn test_parse_path_refs_entry_point() {
        let ws = referencing("\n\n\n\n\n\nfn f(_: other_crate::MyStruct) {}");
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "", Some("MyStruct"))]
        );
        assert_eq!(ws.deps("my_crate")[0].line, 7);
    }

    #[test]
    fn test_parse_path_refs_dedup() {
        let ws = referencing(
            "fn main() {\n    other_crate::module::item();\n    other_crate::module::item();\n}",
        );
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "module", Some("item"))],
            "duplicate paths should be deduped"
        );
    }
}

mod relative_path_tests {
    use super::*;

    #[test]
    fn test_super_basic() {
        // super::foo from module "a::b", depth 0 → "a::foo"
        let result = resolve_relative_path("super::foo", "a::b");
        assert_eq!(result, Some("a::foo".to_string()));
    }

    #[test]
    fn test_super_from_root() {
        // super::foo from crate root (empty path) → None (no parent)
        let result = resolve_relative_path("super::foo", "");
        assert_eq!(result, None);
    }

    #[test]
    fn test_super_super() {
        // super::super::foo from "a::b::c" → "a::foo"
        let result = resolve_relative_path("super::super::foo", "a::b::c");
        assert_eq!(result, Some("a::foo".to_string()));
    }

    #[test]
    fn test_super_too_many() {
        // super::super::foo from "a" → None (can't go above root)
        let result = resolve_relative_path("super::super::foo", "a");
        assert_eq!(result, None);
    }

    #[test]
    fn test_self_basic() {
        // self::child from "render", depth 0 → "render::child"
        let result = resolve_relative_path("self::child", "render");
        assert_eq!(result, Some("render::child".to_string()));
    }

    #[test]
    fn test_self_from_root() {
        // self::foo from crate root → "foo"
        let result = resolve_relative_path("self::foo", "");
        assert_eq!(result, Some("foo".to_string()));
    }
}

mod relative_import_e2e_tests {
    use super::*;

    #[test]
    fn test_super_filtering_from_workspace() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod analyze;"),
                    (
                        "src/analyze/mod.rs",
                        "mod filtering;\nmod use_parser;\nmod workspace;",
                    ),
                    ("src/analyze/filtering.rs", "pub struct DependencyInfo;"),
                    ("src/analyze/use_parser.rs", ""),
                    (
                        "src/analyze/workspace.rs",
                        "use super::filtering::DependencyInfo;",
                    ),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::analyze::workspace"),
            [("my_crate", "analyze::filtering", Some("DependencyInfo"))]
        );
    }

    #[test]
    fn test_super_use_parser_from_syn_walker() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod analyze;"),
                    ("src/analyze/mod.rs", "mod syn_walker;\nmod use_parser;"),
                    (
                        "src/analyze/use_parser.rs",
                        "pub fn parse_workspace_dependencies() {}\npub fn collect_all_use_items() {}",
                    ),
                    (
                        "src/analyze/syn_walker.rs",
                        "use super::use_parser::{parse_workspace_dependencies, collect_all_use_items};",
                    ),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate::analyze::syn_walker"),
            [
                (
                    "my_crate",
                    "analyze::use_parser",
                    Some("collect_all_use_items")
                ),
                (
                    "my_crate",
                    "analyze::use_parser",
                    Some("parse_workspace_dependencies")
                ),
            ]
        );
    }

    #[test]
    fn test_super_from_crate_root_ignored() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod some_mod;\nuse super::some_mod;"),
                    ("src/some_mod.rs", ""),
                ],
            )
            .analyze();
        assert!(
            ws.deps("my_crate").is_empty(),
            "super:: from root should produce no deps: {:?}",
            ws.deps("my_crate")
        );
    }

    #[test]
    fn test_self_child_module() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod render;"),
                    ("src/render/mod.rs", "mod child;\nuse self::child::Item;"),
                    ("src/render/child.rs", "pub struct Item;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::render"),
            [("my_crate", "render::child", Some("Item"))]
        );
    }
}

/// A file under `tests/` is a crate of its own, whose root is that file. The
/// lib declares `support`, `cases` and `cases::harness` as well, so a path
/// read against the lib lands on a lib module instead of failing.
mod integration_test_crate_tests {
    use super::*;

    fn suite(case: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "app",
                &[
                    ("src/lib.rs", "pub mod support;\npub mod cases;"),
                    ("src/support.rs", "pub fn helper() {}"),
                    ("src/cases.rs", "pub mod harness;"),
                    ("src/cases/harness.rs", "pub struct Probe;"),
                    ("tests/suite.rs", "mod support;\nmod cases;"),
                    ("tests/support/mod.rs", "pub fn helper() {}"),
                    ("tests/cases/mod.rs", "mod harness;\nmod user;"),
                    ("tests/cases/harness.rs", "pub struct Probe;"),
                    ("tests/cases/user.rs", case),
                ],
            )
            .analyze_with_tests()
    }

    /// The module each dependency of the module at `path` lands on, with its
    /// item.
    fn landings<'a>(ws: &'a Analyzed, path: &str) -> Vec<(String, Option<&'a str>)> {
        ws.deps(path)
            .iter()
            .map(|dep| (dep.module_target(), dep.target_item.as_deref()))
            .collect()
    }

    #[test]
    fn crate_path_names_the_test_crate() {
        let ws = suite("use crate::support::helper;");
        assert_eq!(
            landings(&ws, "app::tests::suite::cases::user"),
            [("app::tests::suite::support".to_string(), Some("helper"))]
        );
    }

    #[test]
    fn super_path_stays_in_the_test_crate() {
        let ws = suite("use super::harness::Probe;");
        assert_eq!(
            landings(&ws, "app::tests::suite::cases::user"),
            [(
                "app::tests::suite::cases::harness".to_string(),
                Some("Probe")
            )]
        );
    }

    #[test]
    fn package_name_names_the_lib() {
        let ws = suite("use app::support::helper;");
        assert_eq!(
            landings(&ws, "app::tests::suite::cases::user"),
            [("app::support".to_string(), Some("helper"))]
        );
    }

    /// Cargo names the crate of `tests/my-test.rs` `my_test`, and its
    /// modules keep the file name in the tree.
    #[test]
    fn crate_path_in_a_hyphenated_test_file_lands_on_its_module() {
        let ws = TestWorkspace::new()
            .package(
                "app",
                &[
                    ("src/lib.rs", ""),
                    (
                        "tests/my-test.rs",
                        "mod helpers;\nuse crate::helpers::helper;",
                    ),
                    ("tests/helpers/mod.rs", "pub fn helper() {}"),
                ],
            )
            .analyze_with_tests();
        let test_crate = ws
            .module("app")
            .children
            .iter()
            .find(|module| module.name == "my-test")
            .expect("the test crate keeps its file name");
        let helpers = &test_crate.children[0];
        assert_eq!(
            landings(&ws, &test_crate.full_path),
            [(helpers.full_path.clone(), Some("helper"))]
        );
    }
}

mod inline_module_depth_tests {
    use super::*;

    /// Analyze, with test code, a crate whose module `analyze::filtering` is
    /// `source`, beside a module `analyze::sibling` that defines `Item`.
    fn filtering(source: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod analyze;"),
                    ("src/analyze/mod.rs", "mod filtering;\nmod sibling;"),
                    ("src/analyze/filtering.rs", source),
                    ("src/analyze/sibling.rs", "pub struct Item;"),
                ],
            )
            .analyze_with_tests()
    }

    #[test]
    fn test_super_star_in_cfg_test_mod_no_upward_edge() {
        let ws = filtering("fn some_fn() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n}");
        assert!(
            ws.deps("my_crate::analyze::filtering").is_empty(),
            "super::* in mod tests should not create upward edge: {:?}",
            ws.deps("my_crate::analyze::filtering")
        );
    }

    #[test]
    fn test_super_super_from_inline_mod_creates_real_edge() {
        // The path goes above the file's module, so it is a real edge.
        let ws = filtering("#[cfg(test)]\nmod tests {\n    use super::super::sibling::Item;\n}");
        assert_eq!(
            ws.targets("my_crate::analyze::filtering"),
            [("my_crate", "analyze::sibling", Some("Item"))]
        );
    }

    #[test]
    fn super_from_inline_mod_reaches_a_child_module_of_the_file() {
        // `super` leaves the inline module, `backend` is a child of `render`.
        let source = r"
mod backend;
pub mod api {
    pub use super::backend::Api;
    pub use super::*;
    pub use super::helper;
}
";
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod render;"),
                    ("src/render/mod.rs", source),
                    ("src/render/backend.rs", "pub struct Api;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::render"),
            [("my_crate", "render::backend", Some("Api"))],
            "only the child module is a target"
        );
    }

    #[test]
    fn an_inline_module_does_not_see_an_item_a_use_of_its_file_binds() {
        // `Result` in `inner` is std's: the root's `use` binds it in the
        // root only.
        let source = "mod types;
use types::Res as Result;

mod inner {
    pub fn z() -> Result<(), ()> {
        Ok(())
    }
}
";
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[("src/lib.rs", source), ("src/types.rs", "pub struct Res;")],
            )
            .analyze();
        let lines: Vec<usize> = ws
            .deps("my_crate")
            .iter()
            .filter(|dep| dep.target_module == "types")
            .flat_map(|dep| dep.uses.iter().map(|symbol_use| symbol_use.line))
            .collect();
        assert!(!lines.contains(&5), "uses of types at lines {lines:?}");
    }
}

mod context_aware_dedup_tests {
    use super::*;
    use crate::model::{EdgeContext, TestKind};

    fn contexts(deps: &[DependencyRef]) -> Vec<EdgeContext> {
        deps.iter().map(|d| d.context.clone()).collect()
    }

    #[test]
    fn test_same_target_different_context_not_deduped_use() {
        // Production and Test dep on same symbol must both survive dedup
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod graph;\nuse crate::graph::Node;\n\
                         #[cfg(test)]\nfn check() { use crate::graph::Node; }",
                    ),
                    ("src/graph.rs", "pub struct Node;"),
                ],
            )
            .analyze_with_tests();
        let deps = ws.deps("my_crate");
        assert_eq!(
            ws.targets("my_crate"),
            [
                ("my_crate", "graph", Some("Node")),
                ("my_crate", "graph", Some("Node")),
            ],
            "prod + test on same target must not be deduped"
        );
        assert_eq!(
            contexts(deps),
            [EdgeContext::production(), EdgeContext::test(TestKind::Unit)]
        );
    }

    #[test]
    fn test_same_target_different_context_not_deduped_path_ref() {
        // Production and Test dep on same path ref must both survive dedup
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod module;"),
                    ("src/module.rs", "pub fn item() {}"),
                ],
            )
            .package(
                "my_crate",
                &[(
                    "src/lib.rs",
                    "fn main() { other_crate::module::item(); }\n\
                     #[cfg(test)]\nfn check() { other_crate::module::item(); }",
                )],
            )
            .analyze_with_tests();
        assert_eq!(
            contexts(ws.deps("my_crate")),
            [EdgeContext::production(), EdgeContext::test(TestKind::Unit)],
            "prod + test on same target must not be deduped: {:?}",
            ws.deps("my_crate")
        );
    }
}

mod reexport_resolution_tests {
    use super::*;

    /// Analyze `my_crate` whose root is `root`, with a module `parent` that
    /// re-exports `Config` and `Item` from its child modules.
    fn with_parent(root: &str, consumer: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", root),
                    (
                        "src/parent/mod.rs",
                        "pub mod child;\npub mod sibling;\npub use child::Config;\npub use sibling::Item;",
                    ),
                    ("src/parent/child.rs", "pub struct Config;"),
                    ("src/parent/sibling.rs", "pub struct Item;"),
                    ("src/consumer.rs", consumer),
                ],
            )
            .analyze()
    }

    #[test]
    fn without_reexport_map_unchanged() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod parent;\nuse crate::parent::Item;"),
                    ("src/parent/mod.rs", "pub mod sibling;\npub struct Item;"),
                    ("src/parent/sibling.rs", ""),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("my_crate", "parent", Some("Item"))]
        );
    }

    #[test]
    fn super_import_resolved_via_reexport_map() {
        let ws = with_parent("mod consumer;\nmod parent;", "use super::parent::Item;");
        assert_eq!(
            ws.targets("my_crate::consumer"),
            [("my_crate", "parent::sibling", Some("Item"))],
            "should resolve through re-export to sibling"
        );
    }

    #[test]
    fn crate_path_resolved_via_reexport_map() {
        let ws = with_parent("mod consumer;\nmod parent;\nuse crate::parent::Config;", "");
        assert_eq!(
            ws.targets("my_crate"),
            [("my_crate", "parent::child", Some("Config"))],
            "should resolve crate:: path through re-export"
        );
    }

    #[test]
    fn path_ref_resolved_via_reexport_map() {
        let ws = with_parent(
            "mod consumer;\nmod parent;\nfn f(_: crate::parent::Config) {}",
            "",
        );
        assert_eq!(
            ws.targets("my_crate"),
            [("my_crate", "parent::child", Some("Config"))],
            "path refs should also resolve through re-export"
        );
    }

    /// `pub use helper::helper;` next to a private `mod helper;`: a path
    /// ending in that name from another crate names the fn the module
    /// re-exports, not the private module.
    #[test]
    fn path_naming_a_module_and_the_item_its_parent_reexports_takes_the_item() {
        let ws = TestWorkspace::new()
            .package(
                "core_crate",
                &[
                    ("src/lib.rs", "pub mod util;"),
                    ("src/util/mod.rs", "mod helper;\npub use helper::helper;"),
                    ("src/util/helper.rs", "pub fn helper() {}"),
                ],
            )
            .package(
                "my_crate",
                &[(
                    "src/lib.rs",
                    "use core_crate::util::helper;\nfn f() { helper(); }",
                )],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("core_crate", "util::helper", Some("helper"))]
        );
        assert_eq!(ws.deps("my_crate")[0].uses.len(), 1);
    }

    /// `my_crate` with `pub struct Set;` in `store::set`, re-exported by
    /// `store`, and a module `holder` that binds the name `bound` to the
    /// module `store` with `holder_use`. The root holds `root` after its
    /// module declarations; `holder::user` is `user`.
    fn with_a_module_bound_in_holder(holder_use: &str, root: &str, user: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", &format!("mod holder;\nmod store;\n{root}")),
                    ("src/store/mod.rs", "pub mod set;\npub use set::Set;"),
                    ("src/store/set.rs", "pub struct Set;"),
                    ("src/holder/mod.rs", &format!("mod user;\n{holder_use}")),
                    ("src/holder/user.rs", user),
                ],
            )
            .analyze()
    }

    #[test]
    fn path_through_a_private_module_binding_of_an_ancestor_keeps_its_item() {
        let ws = with_a_module_bound_in_holder(
            "use crate::store as bound;",
            "",
            "use super::bound::Set;\nfn f(_: Set) {}",
        );
        assert_eq!(
            ws.targets("my_crate::holder::user"),
            [("my_crate", "store::set", Some("Set"))]
        );
        assert_eq!(ws.deps("my_crate::holder::user")[0].uses.len(), 1);
    }

    #[test]
    fn path_through_a_reexported_module_keeps_its_item() {
        let ws = with_a_module_bound_in_holder(
            "pub use crate::store as bound;",
            "use crate::holder::bound::Set;\nfn f(_: Set) {}",
            "",
        );
        assert_eq!(
            ws.targets("my_crate"),
            [("my_crate", "store::set", Some("Set"))]
        );
        assert_eq!(ws.deps("my_crate")[0].uses.len(), 1);
    }
}

mod binding_tests {
    use super::*;

    fn target(module: &str) -> ReExportTarget {
        ReExportTarget {
            crate_name: "my_crate".to_string(),
            module: module.to_string(),
            original_name: "Name".to_string(),
        }
    }

    fn definition(line: usize) -> BindingKind {
        BindingKind::Definition(Definition {
            kind: DefKind::Struct,
            line,
        })
    }

    /// A map that holds `info` as the module `holder` of `my_crate`, seen
    /// from the lib root of `my_crate`.
    fn holder_map(info: ModuleExportInfo) -> ReExportMap {
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([("holder".to_string(), info)]),
        )]
        .into_iter()
        .collect();
        map.for_root("my_crate", 0)
    }

    const HOLDER: CrateModule<'static> = CrateModule {
        crate_name: "my_crate",
        path: "holder",
    };

    /// The bindings of `holder`, as `holder` itself sees them.
    fn holder(map: &ReExportMap) -> SeenBindings<'_> {
        map.viewer(TargetRoot::AsSeen, "holder")
            .bindings_in(HOLDER)
            .expect("the map holds the module")
    }

    /// The bindings of a name a viewer sees come apart from those it does
    /// not: a binding restricted to `holder` is unseen from outside it.
    #[test]
    fn a_viewer_sees_the_bindings_of_a_name_visible_from_its_module() {
        let public = NameBinding::new(definition(1), Visibility::Public);
        let private_use = NameBinding::new(
            BindingKind::PrivateUse(target("a")),
            Visibility::Restricted("holder".to_string()),
        );
        let mut info = ModuleExportInfo::default();
        info.bind("Name".to_string(), public.clone());
        info.bind("Name".to_string(), private_use.clone());
        let map = holder_map(info);
        let seen_from = |module| {
            map.viewer(TargetRoot::AsSeen, module)
                .bindings_in(HOLDER)
                .expect("the map holds the module")
        };

        let outside = seen_from("other");
        assert_eq!(outside.named("Name").collect::<Vec<_>>(), [&public]);
        assert_eq!(outside.unseen("Name").collect::<Vec<_>>(), [&private_use]);
        let inside = seen_from("holder::kid");
        assert_eq!(
            inside.named("Name").collect::<Vec<_>>(),
            [&public, &private_use]
        );
        assert_eq!(inside.unseen("Name").next(), None);
    }

    /// A viewer through the holder of a glob sees what both see: a binding
    /// restricted to a module only the viewer is in is unseen, and so is
    /// every restricted binding through a holder in another crate.
    #[test]
    fn a_viewer_through_a_holder_sees_what_both_see() {
        let in_a = NameBinding::new(definition(1), Visibility::Restricted("a".to_string()));
        let in_a_b = NameBinding::new(definition(2), Visibility::Restricted("a::b".to_string()));
        let mut info = ModuleExportInfo::default();
        info.bind("Name".to_string(), in_a.clone());
        info.bind("Name".to_string(), in_a_b.clone());
        let map = holder_map(info);
        let viewer = map.viewer(TargetRoot::AsSeen, "a::b::c");
        let through = |crate_name, path| {
            viewer
                .through(CrateModule { crate_name, path })
                .bindings_in(HOLDER)
                .expect("the map holds the module")
        };

        let sibling = through("my_crate", "a::d");
        assert_eq!(sibling.named("Name").collect::<Vec<_>>(), [&in_a]);
        assert_eq!(sibling.unseen("Name").collect::<Vec<_>>(), [&in_a_b]);
        assert_eq!(through("other", "a::b").named("Name").next(), None);
    }

    /// A bin module that globs a module of its own package's lib sees no
    /// `pub(crate)` trait of the lib, which is another crate than the bin:
    /// the glob passes on what a descendant named through it.
    #[test]
    fn a_bin_glob_on_the_own_lib_brings_in_no_trait_of_the_lib_crate() {
        let mut lib_module = ModuleExportInfo::default();
        define(&mut lib_module, "f", DefKind::Fn);
        define_with(
            &mut lib_module,
            "T",
            DefKind::Trait,
            Visibility::Restricted(String::new()),
        );
        let roots = vec![
            RootReExports {
                kind: EntryKind::Lib,
                modules: HashMap::from([("tm".to_string(), lib_module)]),
                test_build_modules: None,
            },
            RootReExports {
                kind: EntryKind::Bin,
                modules: HashMap::from([("holder".to_string(), ModuleExportInfo::default())]),
                test_build_modules: None,
            },
        ];
        let map: ReExportMap = [("my_crate".to_string(), roots)].into_iter().collect();
        let glob = DependencyRef {
            target_package: "my_crate".to_string(),
            target_module: "tm".to_string(),
            target_item: Some("*".to_string()),
            source_file: Path::new("src/holder.rs").to_path_buf(),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
            uses: Vec::new(),
            target_root: TargetRoot::Lib,
        };
        let holder_and_source = (
            ("my_crate".to_string(), "holder".to_string()),
            "tm".to_string(),
        );
        let passed_on = PassedOn {
            globs: HashMap::from([(holder_and_source, true)]),
            uses: HashMap::new(),
        };

        assert!(passed_on.passes_on(
            "my_crate",
            "holder",
            &glob,
            &map.for_root("my_crate", 1),
            &ModulePathMap::default(),
        ));
    }

    #[test]
    fn the_bindings_of_a_name_come_definition_first_and_child_module_last() {
        let private = Visibility::Restricted("holder".to_string());
        let reexport = NameBinding::new(BindingKind::ReExport(target("a")), Visibility::Public);
        let private_use = NameBinding::new(BindingKind::PrivateUse(target("b")), private.clone());
        let elsewhere = NameBinding::new(BindingKind::Elsewhere, private);
        let module = NameBinding::new(BindingKind::Module, Visibility::Public);
        let defined = NameBinding::new(definition(1), Visibility::Public);
        let mut info = ModuleExportInfo::default();
        for binding in [&module, &private_use, &reexport, &elsewhere, &defined] {
            info.bind("Name".to_string(), binding.clone());
        }
        define(&mut info, "Other", DefKind::Fn);
        let map = holder_map(info);

        assert_eq!(
            holder(&map).named("Name").collect::<Vec<_>>(),
            [&defined, &reexport, &private_use, &elsewhere, &module]
        );
    }

    #[test]
    fn bindings_merged_from_alternative_files_both_stay_the_later_first() {
        let restricted = Visibility::Restricted("holder".to_string());
        let public_definition = NameBinding::new(definition(1), Visibility::Public);
        let restricted_definition = NameBinding::new(definition(2), restricted.clone());
        let use_of_a = NameBinding::new(BindingKind::PrivateUse(target("a")), restricted.clone());
        let use_of_b = NameBinding::new(BindingKind::PrivateUse(target("b")), restricted);
        let mut first = ModuleExportInfo::default();
        first.bind("Item".to_string(), public_definition.clone());
        first.bind("Name".to_string(), use_of_a.clone());
        let mut second = ModuleExportInfo::default();
        second.bind("Item".to_string(), restricted_definition.clone());
        second.bind("Name".to_string(), use_of_b.clone());
        // Read in both files, the same binding stays one.
        second.bind("Name".to_string(), use_of_a.clone());
        first.merge(second);
        let map = holder_map(first);
        let bindings = holder(&map);

        // The bindings outlive the names they were looked up by.
        let (item, name) = {
            let (item, name) = (String::from("Item"), String::from("Name"));
            (bindings.named(&item), bindings.named(&name))
        };

        assert_eq!(
            item.collect::<Vec<_>>(),
            [&restricted_definition, &public_definition]
        );
        assert_eq!(name.collect::<Vec<_>>(), [&use_of_b, &use_of_a]);
    }
}

mod lookup_tests {
    use super::*;

    #[test]
    fn private_uses_of_one_name_to_different_targets_differ() {
        let target = |module: &str| ReExportTarget {
            crate_name: "my_crate".to_string(),
            module: module.to_string(),
            original_name: "Name".to_string(),
        };
        let restricted = Visibility::Restricted("holder".to_string());
        let use_of_a = NameBinding::new(BindingKind::PrivateUse(target("a")), restricted.clone());
        let use_of_b = NameBinding::new(BindingKind::PrivateUse(target("b")), restricted);

        assert!(private_targets_differ(
            [&use_of_b, &use_of_a].into_iter(),
            |_| true
        ));
    }
}

mod resolve_reexport_tests {
    use super::super::{ModuleExportInfo, ReExportMap};
    use super::*;
    use rstest::rstest;
    use std::collections::HashMap;

    /// Analyze `my_crate` whose root is `root`, beside the further `files`.
    fn my_crate(root: &str, files: &[(&str, &str)]) -> Analyzed {
        let mut all = vec![("src/lib.rs", root)];
        all.extend_from_slice(files);
        TestWorkspace::new().package("my_crate", &all).analyze()
    }

    #[test]
    fn resolve_reexport_of_a_module_ends_on_the_module() {
        let ws = TestWorkspace::new()
            .package(
                "leaf_crate",
                &[
                    ("src/lib.rs", "mod ir;\npub use ir::*;"),
                    ("src/ir.rs", "pub struct Node;"),
                ],
            )
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod api;\nmod backend;\nmod dispatch;\n\
                         pub use backend::custom;\npub use leaf_crate as leaf;",
                    ),
                    ("src/backend/mod.rs", "pub mod custom;"),
                    ("src/backend/custom.rs", "pub use crate::dispatch::*;"),
                    ("src/dispatch.rs", "pub struct Interface;"),
                    ("src/api.rs", "use crate::custom;\nuse crate::leaf;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate::api"),
            [
                ("my_crate", "backend::custom", None),
                ("leaf_crate", "", None)
            ]
        );
    }

    #[test]
    fn resolve_private_use_of_a_module_ends_on_the_module() {
        let ws = my_crate(
            "mod arena;\nmod compact;",
            &[
                ("src/arena.rs", "pub struct HandleSet;"),
                ("src/compact/mod.rs", "mod functions;\nuse crate::arena;"),
                ("src/compact/functions.rs", "use super::arena;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::compact::functions"),
            [("my_crate", "arena", None)]
        );
    }

    /// A descendant that names a name through its ancestor's private `use`
    /// passes that `use` on: the edge is the descendant's, and the ancestor,
    /// which names nothing itself, keeps none.
    #[test]
    fn a_resolution_returns_the_ancestor_whose_private_use_it_passed() {
        let ws = my_crate(
            "mod defs;\nmod holder;",
            &[
                ("src/defs.rs", "pub struct Thing;"),
                ("src/holder/mod.rs", "mod child;\nuse crate::defs::Thing;"),
                (
                    "src/holder/child.rs",
                    "use super::Thing;\nfn f(_: Thing) {}",
                ),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::holder::child"),
            [("my_crate", "defs", Some("Thing"))]
        );
        assert!(
            ws.deps("my_crate::holder").is_empty(),
            "{:?}",
            ws.deps("my_crate::holder")
        );
    }

    #[test]
    fn resolve_noop_when_own_definition() {
        let ws = my_crate(
            "mod render;\nmod user;",
            &[
                ("src/render.rs", "pub struct Widget;"),
                ("src/user.rs", "use crate::render::Widget;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "render", Some("Widget"))]
        );
    }

    #[test]
    fn resolve_explicit_reexport() {
        let ws = my_crate(
            "mod render;\nmod user;",
            &[
                (
                    "src/render/mod.rs",
                    "pub mod elements;\npub use elements::Widget;",
                ),
                ("src/render/elements.rs", "pub struct Widget;"),
                ("src/user.rs", "use crate::render::Widget;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "render::elements", Some("Widget"))]
        );
    }

    #[test]
    fn resolve_transitive_chain() {
        let ws = my_crate(
            "mod middle;\nmod origin;\nmod root;\nmod user;",
            &[
                ("src/root.rs", "pub use crate::middle::Widget;"),
                ("src/middle.rs", "pub use crate::origin::Widget;"),
                ("src/origin.rs", "pub struct Widget;"),
                ("src/user.rs", "use crate::root::Widget;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "origin", Some("Widget"))]
        );
    }

    #[test]
    fn resolve_rename_chain() {
        let ws = my_crate(
            "mod facade;\nmod origin;\nmod user;",
            &[
                ("src/facade.rs", "pub use crate::origin::Original as Alias;"),
                ("src/origin.rs", "pub struct Original;"),
                ("src/user.rs", "use crate::facade::Alias;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "origin", Some("Original"))]
        );
    }

    #[test]
    fn resolve_glob_reexport() {
        let ws = my_crate(
            "mod elements;\nmod render;\nmod user;",
            &[
                ("src/render.rs", "pub use crate::elements::*;"),
                ("src/elements.rs", "pub struct Widget;"),
                ("src/user.rs", "use crate::render::Widget;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "elements", Some("Widget"))]
        );
    }

    #[test]
    fn resolve_glob_then_explicit() {
        let ws = my_crate(
            "mod facade;\nmod middle;\nmod origin;\nmod user;",
            &[
                ("src/facade.rs", "pub use crate::middle::*;"),
                ("src/middle.rs", "pub use crate::origin::Widget;"),
                ("src/origin.rs", "pub struct Widget;"),
                ("src/user.rs", "use crate::facade::Widget;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "origin", Some("Widget"))]
        );
    }

    #[test]
    fn an_item_of_an_extern_block_behind_a_glob_lands_on_its_module() {
        // rustc: `crate::ext_call` is `my_crate::ffi::{{extern}}::ext_call`.
        let ws = my_crate(
            "mod ffi;\nmod user;\npub use ffi::*;",
            &[
                (
                    "src/ffi.rs",
                    "unsafe extern \"C\" {\n    pub fn ext_call();\n}",
                ),
                ("src/user.rs", "use crate::ext_call;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "ffi", Some("ext_call"))]
        );
    }

    #[test]
    fn an_item_of_an_extern_block_hides_a_glob_name_in_its_file() {
        // rustc: `ext_call` in user is `my_crate::user::{{extern}}::ext_call`.
        let ws = my_crate(
            "mod other;\nmod user;",
            &[
                ("src/other.rs", "pub fn ext_call() {}"),
                (
                    "src/user.rs",
                    "use crate::other::*;\nunsafe extern \"C\" {\n    fn ext_call();\n}\nfn f() { unsafe { ext_call() } }",
                ),
            ],
        );
        let items: Vec<_> = ws
            .targets("my_crate::user")
            .into_iter()
            .map(|(_, _, item)| item)
            .collect();
        assert!(!items.contains(&Some("ext_call")), "{items:?}");
    }

    /// An attribute macro on an `extern` block may remove or rewrite its
    /// items, so they bind nothing and the glob's name stays visible; a
    /// built-in attribute leaves the items in place. `m` holds
    /// `pub use other::*;` and the block under `attribute`.
    // rustc, with `drop_item` expanding to nothing: `crate::m::get` is
    // `my_crate::m::other::get`.
    #[rstest]
    #[case::an_attribute_macro("#[drop_item]", "m::other")]
    #[case::a_built_in_attribute("#[link(name = \"c\")]", "m")]
    #[case::an_attribute_macro_under_cfg_attr(
        "#[cfg_attr(target_arch = \"wasm32\", drop_item)]",
        "m::other"
    )]
    #[case::an_attribute_macro_under_nested_cfg_attr(
        "#[cfg_attr(unix, cfg_attr(target_arch = \"wasm32\", doc = \"x\", drop_item))]",
        "m::other"
    )]
    #[case::only_built_in_attributes_under_cfg_attr(
        "#[cfg_attr(unix, link(name = \"c\"), doc = \"x\", allow(dead_code))]",
        "m"
    )]
    #[case::only_built_in_attributes_under_nested_cfg_attr(
        "#[cfg_attr(unix, cfg_attr(windows, link(name = \"c\")), allow(dead_code))]",
        "m"
    )]
    fn an_extern_block_binds_its_items_unless_an_attribute_macro_stands_on_it(
        #[case] attribute: &str,
        #[case] expected: &str,
    ) {
        let m = format!(
            "mod other;\npub use other::*;\n{attribute}\nunsafe extern \"C\" {{\n    pub fn get() -> i32;\n}}"
        );
        let ws = my_crate(
            "pub mod m;\npub mod user;",
            &[
                ("src/m.rs", &m),
                ("src/m/other.rs", "pub fn get() -> i32 { 1 }"),
                ("src/user.rs", "pub fn f() -> i32 { crate::m::get() }"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", expected, Some("get"))]
        );
    }

    /// An attribute macro on one item of an `extern` block may remove or
    /// rewrite that item, so it binds nothing; a built-in attribute leaves it.
    // rustc, with `drop_item` expanding to nothing: `crate::m::get` is
    // `my_crate::m::other::get`.
    #[rstest]
    #[case::an_attribute_macro("#[drop_item]", "m::other")]
    #[case::an_attribute_macro_under_cfg_attr("#[cfg_attr(unix, drop_item)]", "m::other")]
    #[case::a_built_in_attribute("#[link_name = \"c_get\"]", "m")]
    fn an_extern_item_binds_unless_an_attribute_macro_stands_on_it(
        #[case] attribute: &str,
        #[case] expected: &str,
    ) {
        let m = format!(
            "mod other;\npub use other::*;\nunsafe extern \"C\" {{\n    {attribute}\n    pub fn get() -> i32;\n}}"
        );
        let ws = my_crate(
            "pub mod m;\npub mod user;",
            &[
                ("src/m.rs", &m),
                ("src/m/other.rs", "pub fn get() -> i32 { 1 }"),
                ("src/user.rs", "pub fn f() -> i32 { crate::m::get() }"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", expected, Some("get"))]
        );
    }

    #[test]
    fn an_extern_item_under_an_attribute_macro_leaves_a_glob_name_in_its_file() {
        let ws = my_crate(
            "mod other;\nmod user;",
            &[
                ("src/other.rs", "pub fn ext_call() {}"),
                (
                    "src/user.rs",
                    "use crate::other::*;\nunsafe extern \"C\" {\n    #[drop_item]\n    fn ext_call();\n}\nfn f() { ext_call() }",
                ),
            ],
        );
        assert!(
            ws.targets("my_crate::user")
                .contains(&("my_crate", "other", Some("ext_call"))),
            "{:?}",
            ws.targets("my_crate::user")
        );
    }

    #[test]
    fn an_extern_block_under_an_attribute_macro_leaves_a_glob_name_in_its_file() {
        let ws = my_crate(
            "mod other;\nmod user;",
            &[
                ("src/other.rs", "pub fn ext_call() {}"),
                (
                    "src/user.rs",
                    "use crate::other::*;\n#[drop_item]\nunsafe extern \"C\" {\n    fn ext_call();\n}\nfn f() { ext_call() }",
                ),
            ],
        );
        assert!(
            ws.targets("my_crate::user")
                .contains(&("my_crate", "other", Some("ext_call"))),
            "{:?}",
            ws.targets("my_crate::user")
        );
    }

    #[test]
    fn an_external_item_passed_on_behind_a_glob_lands_on_the_module_passing_it_on() {
        // rustc: `crate::Error` is `core::fmt::Error`, reached through
        // `error`, the last module of the crate on the way.
        let ws = my_crate(
            "mod error;\nmod viafile;\nmod viaroot;\npub(crate) use error::*;",
            &[
                ("src/error.rs", "pub use std::fmt::Error;"),
                ("src/viafile.rs", "use crate::error::Error;"),
                ("src/viaroot.rs", "use crate::Error;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::viafile"),
            [("my_crate", "error", Some("Error"))]
        );
        assert_eq!(
            ws.targets("my_crate::viaroot"),
            [("my_crate", "error", Some("Error"))]
        );
    }

    #[test]
    fn a_path_from_a_name_a_glob_brings_from_outside_may_name_a_crate() {
        // rustc: a glob that brings `helper` only as a macro leaves the path
        // `helper::Thing` to the crate `helper` (probe: a glob bringing `core`
        // as a macro leaves `core::fmt::Error` to the crate). Which namespace
        // `helper::helper` is in, the map does not know.
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod reexports;\nmod user;\npub use reexports::*;",
                    ),
                    ("src/reexports.rs", "pub use helper::helper;"),
                    ("src/user.rs", "use crate::*;\nfn f(_: helper::Thing) {}"),
                ],
            )
            .analyze();
        let items: Vec<_> = ws
            .targets("my_crate::user")
            .into_iter()
            .map(|(_, _, item)| item)
            .collect();
        assert!(!items.contains(&Some("helper")), "{items:?}");
    }

    #[test]
    fn a_path_from_a_crate_a_glob_brings_goes_through_the_glob() {
        // rustc: where a glob brings a module `core`, `core::Marker` is read
        // through it, not from the extern prelude's crate `core` (probe). A
        // `pub use` of a lone crate name binds a crate, which is a module.
        let ws = my_crate(
            "mod user;\npub use helper;",
            &[("src/user.rs", "use crate::*;\nfn f(_: helper::Thing) {}")],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "", Some("helper"))]
        );
    }

    /// `make_flags!` defines a unit struct of the name it is given.
    const MAKE_FLAGS: &str = "macro_rules! make_flags { ($n:ident) => { pub struct $n; } }";

    #[test]
    fn a_name_only_a_macro_defines_lands_on_the_one_glob_source_calling_a_macro() {
        // rustc: `kinds::Caps` is `kinds::flags::Caps`.
        let flags = format!("{MAKE_FLAGS}\nmake_flags!(Caps);\npub struct Plain;");
        let ws = TestWorkspace::new()
            .package(
                "kinds",
                &[
                    ("src/lib.rs", "mod flags;\npub use flags::*;"),
                    ("src/flags.rs", &flags),
                ],
            )
            .package(
                "reader",
                &[
                    ("src/lib.rs", "mod user;"),
                    ("src/user.rs", "use kinds::{Caps, Plain};"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("reader::user"),
            [
                ("kinds", "flags", Some("Caps")),
                ("kinds", "flags", Some("Plain"))
            ]
        );
    }

    #[test]
    fn a_name_only_a_macro_defines_lands_on_the_macro_caller_down_a_chain_of_globs() {
        // rustc: `crate::outer::Deeper` is `my_crate::outer::middle::deep::Deeper`.
        let deep = format!("{MAKE_FLAGS}\nmake_flags!(Deeper);");
        let ws = my_crate(
            "mod outer;\nmod user;",
            &[
                ("src/outer/mod.rs", "mod middle;\npub use middle::*;"),
                ("src/outer/middle/mod.rs", "pub mod deep;\npub use deep::*;"),
                ("src/outer/middle/deep.rs", &deep),
                ("src/user.rs", "use crate::outer::Deeper;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "outer::middle::deep", Some("Deeper"))]
        );
    }

    /// A name no binding lists, `Caps`, goes to the one module that may define
    /// it by a macro, and stays on the root where another module may define
    /// it too. `a` calls `make_flags!` with `a_makes`; the root holds `root`
    /// beside `pub use a::*;` and `pub use b::*;`, and `b` holds `b`.
    #[rstest]
    #[case::b_calls_a_macro_too("", "Caps", "make_flags!(Other);", "")]
    #[case::the_root_calls_a_macro_too(
        &format!("{MAKE_FLAGS}\nmake_flags!(Other);"),
        "Caps",
        "",
        ""
    )]
    #[case::b_carries_a_derive_macro("", "Caps", "#[derive(Widget)]\npub struct Other;", "")]
    #[case::b_carries_an_attribute_macro("", "Caps", "#[make_more]\npub fn other() {}", "")]
    #[case::b_carries_only_built_in_attributes(
        "",
        "Caps",
        "#[derive(Debug, Clone)]\n#[cfg_attr(test, derive(Default))]\npub struct Other;\n#[inline]\npub fn other() {}",
        "a"
    )]
    #[case::the_root_holds_a_glob_of_another_crate("pub use std::collections::*;", "Caps", "", "")]
    #[case::b_defines_a_union_of_the_name("", "Other", "pub union Caps { f: u8 }", "")]
    #[case::the_root_binds_the_name_to_a_crate("extern crate alloc as Caps;", "Caps", "", "")]
    #[case::b_exports_a_macro_of_the_name(
        "",
        "Other",
        "#[macro_export]\nmacro_rules! Caps { () => {} }",
        ""
    )]
    fn a_name_only_a_macro_defines_goes_to_the_one_module_that_may_define_it(
        #[case] root: &str,
        #[case] a_makes: &str,
        #[case] b: &str,
        #[case] expected: &str,
    ) {
        let a = format!("{MAKE_FLAGS}\nmake_flags!({a_makes});");
        let b = format!("{MAKE_FLAGS}\n{b}");
        let ws = my_crate(
            &format!("mod a;\nmod b;\nmod user;\npub use a::*;\npub use b::*;\n{root}"),
            &[
                ("src/a.rs", &a),
                ("src/b.rs", &b),
                ("src/user.rs", "use crate::Caps;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", expected, Some("Caps"))]
        );
    }

    #[test]
    fn a_macro_passed_on_by_its_own_module_behind_a_glob_lands_on_that_module() {
        // rustc: `crate::shout` is the `macro_rules! shout` of `my_crate::mac`.
        let ws = my_crate(
            "mod a;\nmod mac;\nmod user;\npub(crate) use a::*;\npub(crate) use mac::*;",
            &[
                ("src/a.rs", "#[derive(Widget)]\npub struct Other;"),
                (
                    "src/mac.rs",
                    "macro_rules! shout { () => { 1 } }\npub(crate) use shout;",
                ),
                ("src/user.rs", "use crate::shout;"),
            ],
        );
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "mac", Some("shout"))]
        );
    }

    #[test]
    fn a_call_of_a_name_its_module_binds_to_its_own_macro_goes_on_through_the_glob() {
        // rustc: `crate::m::shout()` is the fn `my_crate::m::other::shout`,
        // `crate::m::shout!()` the `macro_rules! shout` of `my_crate::m`.
        let ws = my_crate(
            "pub mod m;\npub mod user;",
            &[
                (
                    "src/m.rs",
                    "mod other;\npub(crate) use other::*;\nmacro_rules! shout { () => { 1 } }\npub(crate) use shout;",
                ),
                ("src/m/other.rs", "pub fn shout() -> i32 { 2 }"),
                (
                    "src/user.rs",
                    "pub fn f() -> i32 { crate::m::shout() }\npub fn g() -> i32 { crate::m::shout!() }",
                ),
            ],
        );
        assert_eq!(
            ws.sorted_targets("my_crate::user"),
            [
                ("my_crate", "m", Some("shout")),
                ("my_crate", "m::other", Some("shout"))
            ]
        );
    }

    /// `mac` binds `shout` to its own macro, `fns` defines the fn `shout`,
    /// both behind globs in that order, `depth` modules below `m`.
    // rustc: `crate::m::shout()` is the fn of `fns`.
    #[rstest]
    #[case::one_glob_down("m")]
    #[case::two_globs_down("m::mid")]
    fn a_call_through_globs_passes_a_glob_source_with_only_a_macro_of_the_name(
        #[case] holder: &str,
    ) {
        let globs = "mod mac;\nmod fns;\npub(crate) use mac::*;\npub(crate) use fns::*;";
        let mac = format!("src/{}/mac.rs", holder.replace("::", "/"));
        let fns = format!("src/{}/fns.rs", holder.replace("::", "/"));
        let mut files = vec![
            (
                mac.as_str(),
                "macro_rules! shout { () => { 1 } }\npub(crate) use shout;",
            ),
            (fns.as_str(), "pub fn shout() -> i32 { 2 }"),
            ("src/user.rs", "pub fn f() -> i32 { crate::m::shout() }"),
        ];
        if holder == "m" {
            files.push(("src/m.rs", globs));
        } else {
            files.push(("src/m.rs", "mod mid;\npub(crate) use mid::*;"));
            files.push(("src/m/mid.rs", globs));
        }
        let ws = my_crate("pub mod m;\npub mod user;", &files);
        let expected = format!("{holder}::fns");
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", expected.as_str(), Some("shout"))]
        );
    }

    #[test]
    fn an_attribute_names_a_macro_through_globs() {
        // rustc: `#[make]` in `parts::piece` is the attribute macro of `macros`.
        let ws = TestWorkspace::new()
            .package(
                "macros",
                &[(
                    "src/lib.rs",
                    "#[proc_macro_attribute]\npub fn make(_: TokenStream, input: TokenStream) -> TokenStream { input }",
                )],
            )
            .package(
                "app",
                &[
                    ("src/lib.rs", "mod parts;"),
                    ("src/parts/mod.rs", "use macros::*;\nmod piece;\npub use piece::*;"),
                    ("src/parts/piece.rs", "use super::*;\n#[make]\npub fn piece() {}"),
                ],
            )
            .analyze();
        assert!(
            ws.targets("app::parts::piece")
                .contains(&("macros", "", Some("make"))),
            "{:?}",
            ws.targets("app::parts::piece")
        );
    }

    /// A derive macro may declare helper attributes, which name no macro: a
    /// bare attribute on an item that derives a macro, or on its fields and
    /// variants, is not read. `m` binds `shout` to its own macro behind a glob.
    // rustc: `#[shout]` under `#[derive(Helped)]` is the derive's helper.
    #[rstest]
    #[case::on_a_field("#[derive(Helped)]\npub struct S {\n    #[shout]\n    pub x: u8,\n}")]
    #[case::on_a_variant("#[derive(Helped)]\npub enum E {\n    #[shout]\n    A,\n}")]
    #[case::on_the_item("#[derive(Helped)]\n#[shout]\npub struct S;")]
    #[case::under_cfg_attr(
        "#[cfg_attr(unix, derive(Helped))]\npub struct S {\n    #[shout]\n    pub x: u8,\n}"
    )]
    fn a_bare_attribute_under_a_derive_macro_names_nothing(#[case] item: &str) {
        let user = format!("use crate::m::*;\n{item}");
        let ws = my_crate(
            "pub mod m;\npub mod user;",
            &[
                ("src/m.rs", "mod mac;\npub(crate) use mac::*;"),
                (
                    "src/m/mac.rs",
                    "macro_rules! shout { () => { 1 } }\npub(crate) use shout;",
                ),
                ("src/user.rs", &user),
            ],
        );
        let items: Vec<_> = ws
            .targets("my_crate::user")
            .into_iter()
            .map(|(_, _, item)| item)
            .collect();
        assert!(!items.contains(&Some("shout")), "{items:?}");
    }

    /// Under built-in derives only, and with more than one segment, an
    /// attribute still names a macro.
    #[rstest]
    #[case::built_in_derives_only(
        "#[derive(Debug, Clone)]\npub struct S {\n    #[shout]\n    pub x: u8,\n}"
    )]
    #[case::a_path_under_a_derive_macro(
        "#[derive(Helped)]\npub struct S {\n    #[crate::m::shout]\n    pub x: u8,\n}"
    )]
    fn an_attribute_outside_a_derive_helpers_reach_names_a_macro(#[case] item: &str) {
        let user = format!("use crate::m::*;\n{item}");
        let ws = my_crate(
            "pub mod m;\npub mod user;",
            &[
                ("src/m.rs", "mod mac;\npub(crate) use mac::*;"),
                (
                    "src/m/mac.rs",
                    "macro_rules! shout { () => { 1 } }\npub(crate) use shout;",
                ),
                ("src/user.rs", &user),
            ],
        );
        assert!(
            ws.targets("my_crate::user")
                .contains(&("my_crate", "m::mac", Some("shout"))),
            "{:?}",
            ws.targets("my_crate::user")
        );
    }

    #[test]
    fn resolve_cycle_guard() {
        let ws = my_crate(
            "mod a;\nmod b;\nmod user;",
            &[
                ("src/a.rs", "pub use crate::b::Widget;"),
                ("src/b.rs", "pub use crate::a::Widget;"),
                ("src/user.rs", "use crate::a::Widget;"),
            ],
        );
        // Terminates without infinite loop; ends back at "a"
        assert_eq!(
            ws.targets("my_crate::user"),
            [("my_crate", "a", Some("Widget"))]
        );
    }

    #[test]
    fn resolve_cross_crate() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "mod settings;\npub use settings::Config;"),
                    ("src/settings.rs", "pub struct Config;"),
                ],
            )
            .package("my_crate", &[("src/lib.rs", "use other_crate::Config;")])
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("other_crate", "settings", Some("Config"))]
        );
    }

    /// `front::wgsl` holds a private `use self::error::Error`, and the module
    /// `user` holds `user`.
    fn with_private_use_in_wgsl(user: &str) -> Analyzed {
        my_crate(
            "mod back;\nmod front;",
            &[
                ("src/front/mod.rs", "mod wgsl;"),
                (
                    "src/front/wgsl/mod.rs",
                    "mod error;\nmod parse;\nuse self::error::Error;",
                ),
                ("src/front/wgsl/error.rs", "pub enum Error {}"),
                ("src/front/wgsl/parse/mod.rs", "mod directive;"),
                (
                    "src/front/wgsl/parse/directive/mod.rs",
                    "mod enable_extension;",
                ),
                ("src/front/wgsl/parse/directive/enable_extension.rs", user),
                ("src/back/mod.rs", "mod spv;"),
                ("src/back/spv.rs", user),
            ],
        )
    }

    #[test]
    fn resolve_private_use_for_descendant() {
        // A descendant module names it through the ancestor. The edge belongs
        // to the definer.
        let ws = with_private_use_in_wgsl("use crate::front::wgsl::Error;");
        assert_eq!(
            ws.targets("my_crate::front::wgsl::parse::directive::enable_extension"),
            [("my_crate", "front::wgsl::error", Some("Error"))]
        );
    }

    #[test]
    fn resolve_private_use_not_for_non_descendant() {
        // A module outside front::wgsl cannot see its private `use`; without a
        // visible binding the edge stays on the ancestor.
        let ws = with_private_use_in_wgsl("use crate::front::wgsl::Error;");
        assert_eq!(
            ws.targets("my_crate::back::spv"),
            [("my_crate", "front::wgsl", Some("Error"))]
        );
    }

    /// `api` holds a private `use actions::*` and names nothing from it;
    /// `actions` defines `Mapping`.
    fn with_private_glob_in_api(buffer: &str, view: &str, backend: &str) -> Analyzed {
        my_crate(
            "mod api;\nmod backend;",
            &[
                (
                    "src/api/mod.rs",
                    "mod actions;\nmod buffer;\nuse actions::*;",
                ),
                ("src/api/actions.rs", "pub struct Mapping;"),
                ("src/api/buffer/mod.rs", &format!("mod view;\n{buffer}")),
                ("src/api/buffer/view.rs", view),
                ("src/backend.rs", backend),
            ],
        )
    }

    #[test]
    fn resolve_private_glob_for_descendant() {
        // A descendant names Mapping through the ancestor. The edge belongs to
        // the definer.
        let ws = with_private_glob_in_api("use super::Mapping;", "", "");
        assert_eq!(
            ws.targets("my_crate::api::buffer"),
            [("my_crate", "api::actions", Some("Mapping"))]
        );
    }

    #[test]
    fn resolve_private_glob_not_for_non_descendant() {
        // A module outside api cannot see its private glob; without a visible
        // binding the edge stays on the ancestor.
        let ws = with_private_glob_in_api("", "", "use crate::api::Mapping;");
        assert_eq!(
            ws.targets("my_crate::backend"),
            [("my_crate", "api", Some("Mapping"))]
        );
    }

    #[test]
    fn resolve_private_glob_chain_for_descendant() {
        // api::buffer forwards api's scope with `use super::*`, api forwards
        // actions with `use actions::*`; a module below api::buffer reaches
        // the definer through both.
        let ws = with_private_glob_in_api("use super::*;", "use super::Mapping;", "");
        assert_eq!(
            ws.targets("my_crate::api::buffer::view"),
            [("my_crate", "api::actions", Some("Mapping"))]
        );
    }

    #[test]
    fn resolve_pub_glob_carries_none_of_the_sources_private_globs() {
        // dispatch holds private globs on custom and core; custom republishes
        // dispatch with `pub use dispatch::*`. That carries dispatch's public
        // items only, so Mapping (defined in core) is not custom's to give,
        // even to dispatch itself.
        let ws = my_crate(
            "mod core;\nmod custom;\nmod dispatch;",
            &[
                ("src/core.rs", "pub struct Mapping;"),
                ("src/custom.rs", "pub use crate::dispatch::*;"),
                (
                    "src/dispatch.rs",
                    "use crate::core::*;\nuse crate::custom::*;\nfn f(_: crate::custom::Mapping) {}",
                ),
            ],
        );
        assert_eq!(
            ws.dep_on("my_crate::dispatch", "Mapping").target_module,
            "custom"
        );
    }

    // --- ModuleExportInfo default ---

    #[test]
    fn is_empty_when_default() {
        let info = ModuleExportInfo::default();
        assert!(info.is_empty());
    }

    // --- ReExportMap from_iter ---

    #[test]
    fn from_iterator_of_modules_makes_a_lib_root() {
        let mut inner = HashMap::new();
        inner.insert("render".to_string(), ModuleExportInfo::default());
        let map: ReExportMap = [("my_crate".to_string(), inner)].into_iter().collect();
        assert!(map.get("my_crate").unwrap().contains_key("render"));
    }
}

mod external_crate_tests {
    use super::*;

    #[test]
    fn test_resolve_external_crate_import() {
        let ws = TestWorkspace::new()
            .package("my_crate", &[("src/lib.rs", "use serde::Deserialize;")])
            .external("my_crate", "serde", "serde-pkg-id")
            .analyze();
        let deps = ws.deps("my_crate");
        assert_eq!(
            deps.len(),
            1,
            "should resolve external crate import: {deps:?}"
        );
        assert_eq!(deps[0].target_package, "serde");
        assert_eq!(deps[0].target_item.as_deref(), Some("Deserialize"));
    }

    #[test]
    fn test_resolve_external_does_not_shadow_workspace() {
        // With serde as both workspace crate and external, workspace should win
        let ws = TestWorkspace::new()
            .package("serde", &[("src/lib.rs", "pub struct Foo;")])
            .package("my_crate", &[("src/lib.rs", "use serde::Foo;")])
            .external("my_crate", "serde", "serde-pkg-id")
            .analyze();
        assert_eq!(ws.targets("my_crate"), [("serde", "", Some("Foo"))]);
    }

    #[test]
    fn test_resolve_external_no_match() {
        let ws = TestWorkspace::new()
            .package("my_crate", &[("src/lib.rs", "use unknown_crate::Foo;")])
            .analyze();
        assert!(
            ws.deps("my_crate").is_empty(),
            "unknown crate should not resolve"
        );
    }
}

/// `use crate::parent::child` binds `child` locally; later `child::Item` paths
/// must resolve to `parent::child`, not to a top-level `child`.
mod file_binding_tests {
    use super::*;
    use rstest::rstest;

    /// Analyze `my_crate` with the modules `parent::child::inner` and a
    /// top-level `child`, and the module `consumer` written as `consumer`.
    fn consuming(consumer: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod child;\nmod consumer;\nmod parent;"),
                    ("src/child.rs", "pub struct Item;"),
                    ("src/parent/mod.rs", "pub mod child;\npub struct Other;"),
                    (
                        "src/parent/child/mod.rs",
                        "pub mod inner;\npub struct Item;",
                    ),
                    ("src/parent/child/inner.rs", "pub struct Item;"),
                    ("src/consumer.rs", consumer),
                ],
            )
            .analyze()
    }

    #[test]
    fn test_alias_map_binds_group_member() {
        // `Other` is no module, so `Other::new` is a path into the item.
        let ws =
            consuming("use crate::parent::{child, Other};\nfn f(_: child::Item) { Other::new(); }");
        assert_eq!(
            ws.sorted_targets("my_crate::consumer"),
            [
                ("my_crate", "parent", Some("Other")),
                ("my_crate", "parent::child", Some("Item")),
            ]
        );
    }

    #[test]
    fn test_alias_map_uses_rename_as_key() {
        let ws = consuming("use crate::parent::child as kid;\nfn f(_: kid::Item) {}");
        assert_eq!(
            ws.sorted_targets("my_crate::consumer"),
            [("my_crate", "parent::child", Some("Item")),],
            "rename binds the alias name to the original module"
        );
    }

    #[test]
    fn test_alias_map_binds_workspace_module() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod module;"),
                    ("src/module.rs", "pub struct Item;"),
                ],
            )
            .package(
                "my_crate",
                &[(
                    "src/lib.rs",
                    "use other_crate::module;\nfn f(_: module::Item) {}",
                )],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate"),
            [("other_crate", "module", Some("Item")),]
        );
    }

    #[test]
    fn test_qualified_use_through_alias_resolves() {
        let ws = consuming("use crate::parent::child;\nfn f(_: child::Item) {}");
        assert_eq!(
            ws.sorted_targets("my_crate::consumer"),
            [("my_crate", "parent::child", Some("Item")),],
            "alias-qualified path must resolve"
        );
    }

    #[test]
    fn test_qualified_use_through_alias_keeps_deeper_segments() {
        let ws = consuming("use crate::parent::child;\nfn f(_: child::inner::Item) {}");
        assert_eq!(
            ws.sorted_targets("my_crate::consumer"),
            [("my_crate", "parent::child::inner", Some("Item")),]
        );
    }

    #[test]
    fn test_alias_does_not_shadow_real_module() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod child;\nmod parent;\nfn f(_: child::Item) {}",
                    ),
                    ("src/child.rs", "pub struct Item;"),
                    ("src/parent/mod.rs", "pub mod child;"),
                    ("src/parent/child.rs", ""),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("my_crate"),
            [("my_crate", "child", Some("Item"))],
            "top-level module still resolves"
        );
    }

    /// Analyze `my_crate` with the modules `consumer` and `shared`, where
    /// `consumer` is `consumer` and the crate reaches the external crate
    /// `remote_lib` when `external` is set.
    fn colliding(consumer: &str, external: bool) -> Analyzed {
        let ws = TestWorkspace::new().package(
            "my_crate",
            &[
                ("src/lib.rs", "mod consumer;\nmod shared;"),
                ("src/shared.rs", "pub struct Item;"),
                ("src/consumer.rs", consumer),
            ],
        );
        if external {
            ws.external("my_crate", "remote_lib", "remote_lib 1.0")
                .analyze()
        } else {
            ws.analyze()
        }
    }

    /// The name can collide through either half of the `use`: its last path
    /// segment or the alias it is renamed to.
    #[rstest]
    #[case::last_segment("use remote_lib::shared;")]
    #[case::alias_name("use remote_lib as shared;")]
    fn test_binding_from_another_crate_beats_same_named_module(#[case] use_line: &str) {
        let ws = colliding(&format!("{use_line}\nfn f(_: shared::Item) {{}}"), true);
        let deps = ws.deps("my_crate::consumer");
        assert!(!deps.is_empty(), "the reference resolves: {deps:?}");
        assert!(
            deps.iter().all(|d| d.target_package == "remote_lib"),
            "the binding names the other crate, not the module beside the file: {deps:?}"
        );
    }

    /// In the crate root, where the module is in scope by its bare name.
    #[test]
    fn test_item_import_from_this_crate_leaves_the_module_alone() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    (
                        "src/lib.rs",
                        "mod helper;\nmod util;\nuse crate::util::helper;\nfn f() { helper::thing(); }",
                    ),
                    ("src/helper.rs", "pub fn thing() {}"),
                    ("src/util.rs", "pub fn helper() {}"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("my_crate"),
            [
                ("my_crate", "helper", Some("thing")),
                ("my_crate", "util", Some("helper")),
            ],
            "an item import does not take the name away from the module"
        );
    }

    /// A `use` binds where it is written. One inside a function body says nothing
    /// about the function beside it, where the bare name still means the module
    /// next to the file.
    #[test]
    fn test_binding_in_a_function_body_leaves_the_neighbour_module_alone() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod metrics;\nmod render;"),
                    ("src/metrics/mod.rs", "pub mod queue;"),
                    ("src/metrics/queue.rs", "pub fn len() {}"),
                    (
                        "src/render/mod.rs",
                        "mod queue;\nfn draw() { queue::submit(); }\n\
                         fn stats() { use crate::metrics::queue; queue::len(); }",
                    ),
                    ("src/render/queue.rs", "pub fn submit() {}"),
                ],
            )
            .analyze();
        let deps = ws.deps("my_crate::render");
        let targets: Vec<&str> = deps.iter().map(|d| d.target_module.as_str()).collect();
        assert!(
            targets.contains(&"render::queue"),
            "the binding in `stats` must not take the name away from `draw`: {deps:?}"
        );
        assert!(
            targets.contains(&"metrics::queue"),
            "the binding still holds in the body it was written in: {deps:?}"
        );
    }

    #[test]
    fn test_unplaceable_binding_beats_same_named_module() {
        let ws = colliding("use remote_lib::shared;\nfn f(_: shared::Item) {}", false);
        let deps = ws.deps("my_crate::consumer");
        assert!(
            deps.is_empty(),
            "the name is bound even where the binding cannot be placed, \
             and the module beside the file is not what it names: {deps:?}"
        );
    }

    /// A `use` path may start with a name another `use` of the file binds.
    #[rstest]
    #[case::at_file_level("use crate::api;\nuse api::schema::Reason;")]
    #[case::in_a_function_body("use crate::api;\nfn f() { use api::schema::Reason; }")]
    fn test_use_path_through_a_bound_module_resolves(#[case] source: &str) {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod api;\nmod server;"),
                    ("src/api/mod.rs", "pub mod schema;"),
                    ("src/api/schema.rs", "pub struct Reason;"),
                    ("src/server.rs", source),
                ],
            )
            .analyze();
        let deps = ws.deps("my_crate::server");
        assert!(
            deps.iter().any(|d| d.target_module == "api::schema"
                && d.target_item.as_deref() == Some("Reason")
                && d.line == 2),
            "the import names `Reason` in `api::schema`: {deps:?}"
        );
    }

    /// A glob brings in the modules of the module it imports from, private
    /// ones included where the file sits below it.
    #[rstest]
    #[case::in_a_path("use crate::*;\nstruct Adapter { inner: dispatch::DispatchAdapter }")]
    #[case::in_a_use("use crate::*;\nuse dispatch::DispatchAdapter;")]
    fn test_path_through_a_module_from_a_glob_resolves(#[case] source: &str) {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod api;\nmod dispatch;\nstruct Config;"),
                    ("src/api/mod.rs", "mod adapter;"),
                    ("src/api/adapter.rs", source),
                    ("src/dispatch.rs", "pub struct DispatchAdapter;"),
                ],
            )
            .analyze();
        let deps = ws.deps("my_crate::api::adapter");
        assert!(
            deps.iter().any(|d| d.target_module == "dispatch"
                && d.target_item.as_deref() == Some("DispatchAdapter")),
            "the path names `DispatchAdapter` in `dispatch`: {deps:?}"
        );
    }

    /// A glob brings in the modules its module binds by `use`: by `pub use`,
    /// and by a private `use` only where the file sits below that module.
    #[rstest]
    #[case::private_use_of_an_ancestor("render::writer", "use crate::shared;", true)]
    #[case::pub_use("other", "pub use crate::shared;", true)]
    #[case::private_use_seen_from_outside("other", "use crate::shared;", false)]
    fn test_path_through_a_module_a_glob_brings_by_use_resolves(
        #[case] consumer: &str,
        #[case] render_use: &str,
        #[case] resolves: bool,
    ) {
        let source = "use crate::render::*;\nfn f() {\n    use shared::flow::Exit;\n}";
        let file_of = |module: &str| if consumer == module { source } else { "" };
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod other;\nmod render;\nmod shared;"),
                    ("src/render/mod.rs", &format!("mod writer;\n{render_use}")),
                    ("src/render/writer.rs", file_of("render::writer")),
                    ("src/other.rs", file_of("other")),
                    ("src/shared/mod.rs", "pub mod flow;"),
                    ("src/shared/flow.rs", "pub struct Exit;"),
                ],
            )
            .analyze();
        let deps = ws.deps(&format!("my_crate::{consumer}"));
        assert_eq!(
            deps.iter().any(|d| d.target_module == "shared::flow"
                && d.target_item.as_deref() == Some("Exit")
                && d.line == 3),
            resolves,
            "whether the import names `Exit` in `shared::flow`: {deps:?}"
        );
    }

    /// A name a `pub use` binds and the module goes on to use is a use of
    /// the item, so the edge is no longer a pure re-export.
    #[test]
    fn test_reexported_name_the_module_uses_is_not_a_pure_reexport() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod value;"),
                    (
                        "src/value/mod.rs",
                        "mod record;\npub use record::Record;\npub enum Value { Record(Record) }",
                    ),
                    ("src/value/record.rs", "pub struct Record;"),
                ],
            )
            .analyze();
        let deps = ws.deps("my_crate::value");
        assert_eq!(
            ws.targets("my_crate::value"),
            [("my_crate", "value::record", Some("Record"))]
        );
        assert!(!deps[0].via_reexport, "the enum uses `Record`: {deps:?}");
    }
}

/// `use path::{self, ...}` imports the module itself — the `self` segment is
/// not an item name.
mod use_self_tests {
    use super::*;

    /// Analyze `my_crate` whose module `dx12` is `source`, beside a module
    /// `auxil` with a child `dxgi`.
    fn dx12(source: &str) -> Analyzed {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod auxil;\nmod dx12;"),
                    ("src/auxil/mod.rs", "pub mod dxgi;\npub struct Thing;"),
                    ("src/auxil/dxgi.rs", "pub struct Factory;"),
                    ("src/dx12/mod.rs", source),
                ],
            )
            .analyze()
    }

    #[test]
    fn test_group_self_imports_module_without_item() {
        let ws = dx12("use crate::auxil::{self, dxgi::Factory};");
        assert_eq!(
            ws.sorted_targets("my_crate::dx12"),
            [
                ("my_crate", "auxil", None),
                ("my_crate", "auxil::dxgi", Some("Factory")),
            ],
            "`self` names the module, not an item"
        );
    }

    #[test]
    fn test_plain_self_import_has_no_item() {
        let ws = dx12("use crate::auxil::self;");
        assert_eq!(
            ws.targets("my_crate::dx12"),
            [("my_crate", "auxil", None)],
            "should resolve to the module"
        );
    }

    #[test]
    fn test_self_rename_keeps_module_as_source() {
        let ws = dx12("use crate::auxil::{self as aux};");
        assert_eq!(
            ws.targets("my_crate::dx12"),
            [("my_crate", "auxil", None)],
            "should resolve to the module"
        );
    }

    #[test]
    fn test_self_module_alias_binds_module() {
        let ws = dx12("use crate::auxil::{self, dxgi::Factory};\nfn f(_: auxil::Thing) {}");
        assert_eq!(
            ws.sorted_targets("my_crate::dx12"),
            [
                ("my_crate", "auxil", Some("Thing")),
                ("my_crate", "auxil::dxgi", Some("Factory")),
            ],
            "`self` binds the module name locally"
        );
    }
}

/// One use per occurrence, each with the category its position and the
/// definition give it.
mod use_category_tests {
    use super::*;
    use crate::model::{SymbolUse, UseCategory};

    /// The module `vocab` with one item of each kind.
    const VOCAB: &str = "pub struct Config { pub x: u8 }
impl Config { pub fn new() -> Self { Config { x: 0 } } pub const MAX: u8 = 1; }
pub enum Mode { Fast, Slow(u8) }
pub const LIMIT: usize = 1;
pub static TABLE: [u8; 0] = [];
pub fn build() {}
pub trait Step { fn run(&self); }
";

    fn deps_of(source: &str) -> Vec<DependencyRef> {
        vocab_user_deps(source, true)
    }

    /// The dependencies of the module `user`, written as `source` beside the
    /// module `vocab`.
    fn vocab_user_deps(source: &str, include_tests: bool) -> Vec<DependencyRef> {
        let ws = TestWorkspace::new().package(
            "my_crate",
            &[
                ("src/lib.rs", "mod user;\nmod vocab;"),
                ("src/vocab.rs", VOCAB),
                ("src/user.rs", source),
            ],
        );
        let analyzed = if include_tests {
            ws.analyze_with_tests()
        } else {
            ws.analyze()
        };
        analyzed.deps("my_crate::user").to_vec()
    }

    fn uses_of<'a>(deps: &'a [DependencyRef], item: &str) -> &'a [SymbolUse] {
        let dep = deps
            .iter()
            .find(|d| d.target_item.as_deref() == Some(item))
            .unwrap_or_else(|| panic!("no dep on {item}: {deps:?}"));
        &dep.uses
    }

    fn use_at(line: usize, category: UseCategory) -> SymbolUse {
        SymbolUse {
            line,
            category,
            imported_for_methods: false,
        }
    }

    /// `my_crate` with a lib root that defines the unit struct `Shared` and a
    /// bin root that defines nothing, seen from the bin root.
    fn bin_root_of_my_crate() -> ReExportMap {
        let mut lib_root = ModuleExportInfo::default();
        define(&mut lib_root, "Shared", DefKind::Struct);
        let roots = vec![
            RootReExports {
                kind: EntryKind::Lib,
                modules: HashMap::from([(String::new(), lib_root)]),
                test_build_modules: None,
            },
            RootReExports {
                kind: EntryKind::Bin,
                modules: HashMap::from([(String::new(), ModuleExportInfo::default())]),
                test_build_modules: None,
            },
        ];
        let map: ReExportMap = [("my_crate".to_string(), roots)].into_iter().collect();
        map.for_root("my_crate", 1)
    }

    /// The dependencies of `banner`, a module of the bin tree of `my_crate`
    /// written as `source`, whose lib root defines the unit struct `Shared`.
    fn bin_tree_deps_of(source: &str) -> Vec<DependencyRef> {
        TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "pub struct Shared;"),
                    ("src/main.rs", "mod banner;\nfn main() {}"),
                    ("src/banner.rs", source),
                ],
            )
            .analyze()
            .deps("my_crate::banner")
            .to_vec()
    }

    /// From the bin root, the package's name stands for the lib root, which
    /// shows the bin tree only what is public; the bin root sees itself.
    #[test]
    fn a_reference_through_the_package_name_sees_the_lib_root_from_outside() {
        let bin_view = bin_root_of_my_crate();
        let in_crate = Visibility::Restricted(String::new());
        let defines_shared = |viewer: Viewer| {
            viewer
                .bindings_in(CrateModule {
                    crate_name: "my_crate",
                    path: "",
                })
                .is_some_and(|root| {
                    root.named("Shared")
                        .any(|binding| binding.definition().is_some())
                })
        };

        let through_name = bin_view.viewer(TargetRoot::Lib, "banner");
        assert!(defines_shared(through_name));
        assert!(!through_name.sees("my_crate", &in_crate));

        let as_seen = bin_view.viewer(TargetRoot::AsSeen, "banner");
        assert!(!defines_shared(as_seen));
        assert!(as_seen.sees("my_crate", &in_crate));
    }

    /// A dependency carries the root it resolves against, so a viewer made
    /// for it reads the module through that root: the lib root for a
    /// reference through the package's name, from the bin tree too.
    #[test]
    fn a_viewer_for_a_dependency_reads_its_module_through_its_root() {
        let bin_view = bin_root_of_my_crate();
        let through_name = DependencyRef {
            target_package: "my_crate".to_string(),
            target_module: String::new(),
            target_item: Some("Shared".to_string()),
            source_file: Path::new("src/banner.rs").to_path_buf(),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
            uses: Vec::new(),
            target_root: TargetRoot::Lib,
        };

        let viewer = bin_view.viewer_for(&through_name, "banner");

        assert!(
            viewer
                .bindings_in(CrateModule::of(&through_name))
                .is_some_and(|root| root.named("Shared").next().is_some())
        );
    }

    /// A file of the bin tree that names its own package names the lib root,
    /// so a bare pattern matches the unit struct the lib root defines.
    #[test]
    fn a_bin_file_naming_its_package_matches_a_lib_root_struct_in_a_pattern() {
        let deps =
            bin_tree_deps_of("use my_crate::Shared;\nfn show(x: Shared) {\n    let Shared = x;\n}");
        assert_eq!(
            uses_of(&deps, "Shared"),
            [use_at(2, UseCategory::Types), use_at(3, UseCategory::Types)]
        );
    }

    /// A glob on the own package from a file of the bin tree brings in what
    /// the lib root defines.
    #[test]
    fn a_bin_file_globbing_its_package_takes_a_lib_root_name() {
        let deps = bin_tree_deps_of("use my_crate::*;\nfn show(_: Shared) {}");
        assert_eq!(uses_of(&deps, "Shared"), [use_at(2, UseCategory::Types)]);
    }

    /// The `use` binds the name; the occurrences are the uses, the import line
    /// is not one.
    #[test]
    fn a_bound_name_in_type_position_is_a_types_use() {
        let deps = deps_of("use crate::vocab::Config;\nfn f(x: Config) -> Option<Config> {}");
        assert_eq!(
            uses_of(&deps, "Config"),
            [use_at(2, UseCategory::Types), use_at(2, UseCategory::Types)]
        );
        assert_eq!(deps.len(), 1, "one dependency, at the import: {deps:?}");
        assert_eq!(deps[0].line, 1);
    }

    /// What a name is used for is decided where it is written: a name a glob
    /// brings in and test code uses is a test use, and the glob, from which no
    /// production code takes a name, keeps its production `*`.
    #[test]
    fn a_glob_name_used_in_test_code_is_a_test_use() {
        let deps = deps_of(
            "use crate::vocab::*;\n#[cfg(test)]\nfn check(_: Config) {}\n#[test]\nfn checks(_: Mode) {}",
        );
        let test = EdgeContext::test(crate::model::TestKind::Unit);
        let contexts: Vec<_> = deps
            .iter()
            .map(|d| (d.target_item.as_deref(), d.context.clone()))
            .collect();
        assert_eq!(
            contexts,
            [
                (Some("*"), EdgeContext::production()),
                (Some("Config"), test.clone()),
                (Some("Mode"), test),
            ]
        );
    }

    /// A name a file-level `use` binds and only test code uses: the import
    /// stays a production dependency, the occurrence is a test use.
    #[test]
    fn a_bound_name_used_in_test_code_is_a_test_use() {
        let deps = deps_of("use crate::vocab::Config;\n#[cfg(test)]\nfn check(_: Config) {}");
        let test = EdgeContext::test(crate::model::TestKind::Unit);
        let test_dep = deps
            .iter()
            .find(|d| d.context == test)
            .unwrap_or_else(|| panic!("no test dependency: {deps:?}"));
        assert_eq!(test_dep.uses, [use_at(3, UseCategory::Types)]);
        let production = deps
            .iter()
            .find(|d| d.context == EdgeContext::production())
            .unwrap_or_else(|| panic!("no production dependency: {deps:?}"));
        assert_eq!(production.line, 1);
        assert!(
            production.uses.iter().all(|u| u.line == 1),
            "the import alone: {deps:?}"
        );
    }

    /// Without test code read, a file's test code adds no dependency, and its
    /// production code resolves as it does with test code read.
    #[test]
    fn without_tests_a_file_yields_its_production_dependencies_only() {
        let source = "use crate::vocab::*;\n#[cfg(test)]\nuse crate::vocab::Mode;\n\
                      fn run(_: Config) {}\n#[cfg(test)]\nfn check(_: Mode) {}";
        let production_with_tests: Vec<_> = vocab_user_deps(source, true)
            .into_iter()
            .filter(|d| d.context == EdgeContext::production())
            .collect();
        assert_eq!(vocab_user_deps(source, false), production_with_tests);
    }

    #[test]
    fn a_free_fn_called_is_a_fns_use() {
        let deps = deps_of("use crate::vocab::build;\nfn f() { build(); crate::vocab::build(); }");
        assert_eq!(
            uses_of(&deps, "build"),
            [use_at(2, UseCategory::Fns), use_at(2, UseCategory::Fns)]
        );
    }

    /// A fn handed on as a value is still behaviour taken from the module.
    #[test]
    fn a_free_fn_passed_as_a_value_is_a_fns_use() {
        let deps = deps_of("fn f() { let g = crate::vocab::build; }");
        assert_eq!(uses_of(&deps, "build"), [use_at(1, UseCategory::Fns)]);
    }

    #[test]
    fn a_const_or_static_read_is_a_values_use() {
        let deps = deps_of(
            "use crate::vocab::{LIMIT, TABLE};\n\
             fn f(n: usize) -> bool { match n { LIMIT => true, _ => n < crate::vocab::LIMIT } }\n\
             fn g() -> usize { TABLE.len() + crate::vocab::TABLE.len() }",
        );
        assert_eq!(
            uses_of(&deps, "LIMIT"),
            [
                use_at(2, UseCategory::Values),
                use_at(2, UseCategory::Values)
            ]
        );
        assert_eq!(
            uses_of(&deps, "TABLE"),
            [
                use_at(3, UseCategory::Values),
                use_at(3, UseCategory::Values)
            ]
        );
    }

    /// `let build = ..` and a parameter `build` are fresh variables even when
    /// a `use` or a glob brings in a fn of that name; a bare identifier
    /// pattern matches only a const, a static or a unit struct.
    #[test]
    fn a_variable_named_like_an_imported_fn_is_no_use() {
        let deps = deps_of(
            "use crate::vocab::{build, Config};\n\
             fn f(build: usize) { let build = 1; let Config = 2; }",
        );
        assert_eq!(uses_of(&deps, "build"), [use_at(1, UseCategory::Fns)]);
        assert_eq!(uses_of(&deps, "Config"), [use_at(2, UseCategory::Types)]);
    }

    /// A pattern that matches a const declares no variable, so the const
    /// stays a use in the arm's body.
    #[test]
    fn a_const_matched_by_a_pattern_stays_visible_in_the_arm() {
        let deps = deps_of(
            "use crate::vocab::LIMIT;\n\
             fn f(n: usize) -> usize { match n { LIMIT => LIMIT, _ => 0 } }",
        );
        assert_eq!(
            uses_of(&deps, "LIMIT"),
            [
                use_at(2, UseCategory::Values),
                use_at(2, UseCategory::Values)
            ]
        );
    }

    /// A republished name is not used here, so a `pub use` carries no use and
    /// no trait is inferred as imported for its methods.
    #[test]
    fn a_reexport_carries_no_use() {
        let deps = deps_of("pub use crate::vocab::{Step, Config};");
        assert!(
            deps.iter().all(|d| d.via_reexport && d.uses.is_empty()),
            "deps: {deps:?}"
        );
    }

    /// A call of a name the crate does not define: a capitalised name is a
    /// tuple-struct or variant constructor, anything else a fn.
    #[test]
    fn a_call_of_an_unknown_name_goes_by_spelling() {
        let source = "fn f() { other::types::Wrapper(1); other::types::make(); other::types::Kind::Variant(2); }";
        let ws = TestWorkspace::new()
            .package(
                "other",
                &[("src/lib.rs", "pub mod types;"), ("src/types.rs", "")],
            )
            .package(
                "my_crate",
                &[("src/lib.rs", "mod user;"), ("src/user.rs", source)],
            )
            .analyze();
        let deps = ws.deps("my_crate::user");
        assert_eq!(uses_of(deps, "Wrapper"), [use_at(1, UseCategory::Types)]);
        assert_eq!(uses_of(deps, "make"), [use_at(1, UseCategory::Fns)]);
        assert_eq!(uses_of(deps, "Kind"), [use_at(1, UseCategory::Types)]);
    }

    /// A struct literal, a tuple-struct call, a variant with and without
    /// fields, and a variant matched: all build or take apart a value of the
    /// type.
    #[test]
    fn a_value_constructed_from_a_struct_or_variant_is_a_types_use() {
        let deps = deps_of(
            "use crate::vocab::{Config, Mode};\n\
             fn f() -> Mode { let c = Config { x: 1 }; let d = crate::vocab::Config(1); Mode::Slow(2) }\n\
             fn g(m: Mode) -> bool { matches!(m, Mode::Fast) && match m { Mode::Slow(_) => true } }",
        );
        assert_eq!(
            uses_of(&deps, "Config"),
            [use_at(2, UseCategory::Types), use_at(2, UseCategory::Types)]
        );
        assert_eq!(
            uses_of(&deps, "Mode"),
            [
                use_at(2, UseCategory::Types),
                use_at(2, UseCategory::Types),
                use_at(3, UseCategory::Types),
                use_at(3, UseCategory::Types),
                use_at(3, UseCategory::Types),
            ],
            "the return type, the constructed variant, the parameter type, the variant inside \
             `matches!`, the matched variant"
        );
    }

    /// `Type::f()` takes behaviour, whether `f` is inherent or a trait fn
    /// named through the trait; `Type::CONST` takes a value.
    #[test]
    fn an_associated_fn_called_is_a_fns_use() {
        let deps = deps_of(
            "use crate::vocab::{Config, Step};\n\
             fn f() { let c = Config::new(); let n = Config::MAX; Step::run(&c); crate::vocab::Config::new(); }",
        );
        assert_eq!(
            uses_of(&deps, "Config"),
            [
                use_at(2, UseCategory::Fns),
                use_at(2, UseCategory::Values),
                use_at(2, UseCategory::Fns),
            ]
        );
        assert_eq!(uses_of(&deps, "Step"), [use_at(2, UseCategory::Fns)]);
    }

    /// An associated item the defining module does not hold: a call is
    /// behaviour, a read goes by the spelling of the name.
    #[test]
    fn an_unknown_associated_item_goes_by_position_then_spelling() {
        let deps = deps_of(
            "use crate::vocab::Config;\n\
             fn f() { Config::parse(); let a = Config::DEFAULT; let b = Config::Builder; }",
        );
        assert_eq!(
            uses_of(&deps, "Config"),
            [
                use_at(2, UseCategory::Fns),
                use_at(2, UseCategory::Values),
                use_at(2, UseCategory::Types),
            ]
        );
    }

    /// Nothing in the file spells the trait's name, so it is there for the
    /// methods it brings into scope. The use sits on the import line.
    #[test]
    fn a_trait_imported_and_never_named_is_a_fns_use_by_inference() {
        let deps = deps_of("use crate::vocab::Step;\nfn f(w: Writer) { w.run(); }");
        assert_eq!(
            uses_of(&deps, "Step"),
            [SymbolUse {
                line: 1,
                category: UseCategory::Fns,
                imported_for_methods: true,
            }]
        );
    }

    #[test]
    fn a_trait_named_is_a_types_use_and_nothing_is_inferred() {
        let deps = deps_of("use crate::vocab::Step;\nimpl Step for Writer {}");
        assert_eq!(uses_of(&deps, "Step"), [use_at(2, UseCategory::Types)]);
    }

    /// A struct, const or fn never named is a dead import; the use it leaves
    /// is what the definition says the item is.
    #[test]
    fn an_import_never_named_is_a_use_by_its_definition() {
        let deps = deps_of("use crate::vocab::{Config, LIMIT, build};");
        assert_eq!(uses_of(&deps, "Config"), [use_at(1, UseCategory::Types)]);
        assert_eq!(uses_of(&deps, "LIMIT"), [use_at(1, UseCategory::Values)]);
        assert_eq!(uses_of(&deps, "build"), [use_at(1, UseCategory::Fns)]);
    }

    /// syn does not parse macro input, so the names in it are read off the
    /// tokens. A bound name is an occurrence, and a qualified path written
    /// there resolves through a binding only: the resolution chain is not
    /// asked, so no edge comes from macro input alone.
    #[test]
    fn a_name_inside_macro_input_is_a_use_through_its_binding() {
        let deps = deps_of(
            "use crate::vocab::{Config, build};\n\
             fn f() { println!(\"{} {}\", build(), Config::MAX); assert!(crate::vocab::LIMIT > 1); }",
        );
        assert_eq!(uses_of(&deps, "build"), [use_at(2, UseCategory::Fns)]);
        assert_eq!(uses_of(&deps, "Config"), [use_at(2, UseCategory::Values)]);
        assert!(
            !deps
                .iter()
                .any(|d| d.target_item.as_deref() == Some("LIMIT")),
            "a qualified path inside macro input is not resolved on its own: {deps:?}"
        );
    }

    #[test]
    fn a_qualified_path_in_type_position_is_a_types_use() {
        let deps = deps_of("fn f(x: crate::vocab::Config) {}");
        assert_eq!(
            uses_of(&deps, "Config"),
            [SymbolUse {
                line: 1,
                category: UseCategory::Types,
                imported_for_methods: false,
            }]
        );
    }
}

// ---------------------------------------------------------------------------
// The test build of a crate root
// ---------------------------------------------------------------------------

mod test_build_tests {
    use super::*;

    /// One lib root defining `name` in module `m` in its build without
    /// `test`, and `test_name` there in its test build.
    fn root_with_test_build(name: &str, test_name: &str) -> Vec<RootReExports> {
        let module = |name: &str| {
            let mut info = ModuleExportInfo::default();
            define(&mut info, name, DefKind::Fn);
            HashMap::from([("m".to_string(), info)])
        };
        vec![RootReExports {
            kind: EntryKind::Lib,
            modules: module(name),
            test_build_modules: Some(module(test_name)),
        }]
    }

    /// The names module `m` of `crate_name` defines, as `viewer` reads them.
    fn names_in(viewer: Viewer, crate_name: &str) -> Vec<String> {
        let mut names: Vec<String> = viewer
            .bindings_in(CrateModule {
                crate_name,
                path: "m",
            })
            .map(|bindings| {
                bindings
                    .all()
                    .filter(|(_, binding)| binding.definition().is_some())
                    .map(|(name, _)| name.to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn the_test_build_sees_its_own_root_tested_and_every_other_crate_without_test() {
        let map: ReExportMap = [
            ("own".to_string(), root_with_test_build("real", "mock")),
            (
                "dep".to_string(),
                root_with_test_build("dep_real", "dep_mock"),
            ),
        ]
        .into_iter()
        .collect();
        let own = map.for_root("own", 0);
        let test_build = own.test_build().expect("own root has a test build");

        let seen = |map: &ReExportMap, root| names_in(map.viewer(root, ""), "own");
        assert_eq!(seen(&own, TargetRoot::AsSeen), ["real"]);
        assert_eq!(seen(&test_build, TargetRoot::AsSeen), ["mock"]);
        let from_test_build = test_build.viewer(TargetRoot::AsSeen, "");
        assert_eq!(names_in(from_test_build, "dep"), ["dep_real"]);
        assert_eq!(seen(&test_build, TargetRoot::Lib), ["mock"]);
        assert_eq!(names_in(from_test_build.reaching("dep"), "own"), ["real"]);
    }

    #[test]
    fn a_root_without_test_build_modules_has_no_test_build() {
        let roots = vec![RootReExports {
            kind: EntryKind::Lib,
            modules: HashMap::new(),
            test_build_modules: None,
        }];
        let map: ReExportMap = [("own".to_string(), roots)].into_iter().collect();
        assert!(map.for_root("own", 0).test_build().is_none());
        assert!(map.test_build().is_none());
    }
}

mod first_segment_tests {
    use super::*;

    /// Analyze `my_crate` whose root is `root`, beside the further `files`,
    /// and return the targets of its module `user`, written as `user`.
    fn user_targets(
        root: &str,
        files: &[(&str, &str)],
        user: &str,
    ) -> Vec<(String, String, Option<String>)> {
        let root = format!("mod user;\n{root}");
        let mut all = vec![("src/lib.rs", root.as_str()), ("src/user.rs", user)];
        all.extend_from_slice(files);
        own(TestWorkspace::new()
            .package("my_crate", &all)
            .analyze()
            .targets("my_crate::user"))
    }

    fn own(targets: Vec<(&str, &str, Option<&str>)>) -> Vec<(String, String, Option<String>)> {
        targets
            .into_iter()
            .map(|(package, module, item)| {
                (
                    package.to_string(),
                    module.to_string(),
                    item.map(String::from),
                )
            })
            .collect()
    }

    fn target(crate_name: &str, module: &str, item: &str) -> (String, String, Option<String>) {
        (
            crate_name.to_string(),
            module.to_string(),
            Some(item.to_string()),
        )
    }

    #[test]
    fn a_private_use_of_a_module_in_the_root_carries_the_path_into_it() {
        let found = user_targets(
            "mod store;\nuse self::store::set;",
            &[
                ("src/store/mod.rs", "pub mod set;"),
                ("src/store/set.rs", "pub struct Set;"),
            ],
            "use crate::set::Set;",
        );
        assert_eq!(found, [target("my_crate", "store::set", "Set")]);
    }

    #[test]
    fn a_private_glob_in_the_root_brings_a_module_into_a_path() {
        let found = user_targets(
            "mod frame;\nuse self::frame::*;",
            &[
                ("src/frame/mod.rs", "pub mod values;"),
                ("src/frame/values.rs", "pub struct Value;"),
            ],
            "use crate::values::Value;",
        );
        assert_eq!(found, [target("my_crate", "frame::values", "Value")]);
    }

    #[test]
    fn a_glob_of_a_module_brings_its_child_module_into_a_path() {
        let found = user_targets(
            "mod holder;\nmod store;",
            &[
                ("src/holder.rs", "pub use crate::store::*;"),
                ("src/store/mod.rs", "pub mod sub;"),
                ("src/store/sub.rs", "pub struct Set;"),
            ],
            "use crate::holder::sub::Set;",
        );
        assert_eq!(found, [target("my_crate", "store::sub", "Set")]);
    }

    #[test]
    fn a_name_bound_to_a_module_through_a_chain_carries_the_path() {
        let found = user_targets(
            "mod holder;\nmod x;\nmod z;",
            &[
                ("src/holder.rs", "pub use crate::x::y as bound;"),
                ("src/x.rs", "pub use crate::z as y;"),
                ("src/z.rs", "pub struct Item;"),
            ],
            "use crate::holder::bound::Item;",
        );
        assert_eq!(found, [target("my_crate", "z", "Item")]);
    }

    #[test]
    fn a_type_in_the_root_hides_a_module_a_glob_brings_under_its_name() {
        // The enum shadows the glob in the type namespace.
        let found = user_targets(
            "mod settings;\npub enum Mode { Fast }\npub use settings::*;",
            &[
                ("src/settings/mod.rs", "pub mod Mode;"),
                ("src/settings/Mode.rs", ""),
            ],
            "use crate::Mode::Fast;",
        );
        assert_eq!(found, [target("my_crate", "", "Mode")]);
    }

    #[test]
    fn an_invisible_definition_does_not_end_the_chain() {
        let found = user_targets(
            "mod m;",
            &[
                (
                    "src/m/mod.rs",
                    "pub mod inner;\n#[cfg(a)]\nfn f() {}\n#[cfg(not(a))]\npub use self::inner::f;",
                ),
                ("src/m/inner.rs", "pub fn f() {}"),
            ],
            "use crate::m::f;",
        );
        assert_eq!(found, [target("my_crate", "m::inner", "f")]);
    }

    /// Analyze `my_crate`, whose module `user` is `user`, beside `other_crate`
    /// whose root is `other_root` and whose module `inner` defines `Thing`.
    fn with_other_crate(other_root: &str, user: &str) -> Vec<(String, String, Option<String>)> {
        own(TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", other_root),
                    ("src/inner.rs", "pub struct Thing;"),
                ],
            )
            .package(
                "my_crate",
                &[("src/lib.rs", "mod user;"), ("src/user.rs", user)],
            )
            .analyze()
            .targets("my_crate::user"))
    }

    #[test]
    fn a_restricted_use_in_another_crates_root_is_no_entry_point() {
        // The `use` binds the fn `Thing` of `inner` for the crate alone; the
        // struct a macro defines in the root is what `other_crate::Thing` names.
        let other_root = r"
mod inner;
pub(crate) use self::inner::Thing;
macro_rules! make {
    ($name:ident) => {
        pub struct $name {}
    };
}
make!(Thing);
";
        let found = own(TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", other_root),
                    ("src/inner.rs", "pub fn Thing() {}"),
                ],
            )
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod user;"),
                    (
                        "src/user.rs",
                        "use other_crate::Thing;\npub fn f(_: Thing) {}",
                    ),
                ],
            )
            .analyze()
            .targets("my_crate::user"));
        assert_eq!(found, [target("other_crate", "", "Thing")]);
    }

    #[test]
    fn a_restricted_use_of_a_module_in_another_crates_root_carries_no_path() {
        let found = with_other_crate(
            "mod inner;\npub(crate) use self::inner as alias;",
            "use other_crate::alias::Thing;",
        );
        assert!(
            !found.contains(&target("other_crate", "inner", "Thing")),
            "{found:?}"
        );
    }
}

mod definition_tests {
    use super::*;

    /// A module takes its definitions from the table at its own
    /// crate-relative path; the crate root reads the `""` entry.
    #[test]
    fn a_module_carries_the_definitions_at_its_own_path() {
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[
                    ("src/lib.rs", "mod graph;\nmod model;\npub struct Root;"),
                    ("src/graph.rs", "\npub struct ArcGraph;"),
                    ("src/model.rs", "use crate::Root;"),
                ],
            )
            .analyze();
        let definition = |line| Definition {
            kind: DefKind::Struct,
            line,
        };

        assert_eq!(
            ws.module("my_crate").definitions.get("Root"),
            Some(&definition(3))
        );
        assert_eq!(
            ws.module("my_crate::graph").definitions.get("ArcGraph"),
            Some(&definition(2))
        );
        assert!(ws.module("my_crate::model").definitions.is_empty());
    }
}
