use super::super::super::test_workspace::TestWorkspace;
use super::*;

mod collect_use_items_tests {
    use super::*;

    #[test]
    fn top_level_use_found() {
        let syntax = syn::parse_file("use foo::Bar;").unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
    }

    #[test]
    fn use_in_fn_body_found() {
        let source = r"
fn main() {
    use foo::Bar;
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1, "use inside fn body must be found");
    }

    #[test]
    fn use_in_nested_block_found() {
        let source = r"
fn main() {
    {
        use foo::Bar;
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1, "use in nested block must be found");
    }

    #[test]
    fn mixed_top_level_and_fn_body() {
        let source = r"
use crate::config;

fn main() {
    use other_crate::utils;
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(
            uses.len(),
            2,
            "both top-level and fn-body uses must be found"
        );
    }

    #[test]
    fn use_in_cfg_block_inside_fn() {
        let source = r#"
fn main() {
    #[cfg(feature = "a")]
    {
        use my_lib::config::Config;
        use my_lib::engine::Engine;
    }
}
"#;
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(
            uses.len(),
            2,
            "uses in cfg-gated block inside fn must be found"
        );
    }

    #[test]
    fn test_collect_use_items_tracks_inline_path() {
        let source = r"
use crate::top;
mod inner {
    use crate::nested;
    mod deep {
        use crate::very_deep;
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 3);
        assert_eq!(&*uses[0].inline_path, "");
        assert_eq!(&*uses[1].inline_path, "inner");
        assert_eq!(&*uses[2].inline_path, "inner::deep");
    }
}

mod cfg_test_scope_tests {
    use super::*;
    use crate::model::EdgeContext;

    #[test]
    fn test_use_in_cfg_test_module_marked() {
        let source = r"
#[cfg(test)]
mod tests {
    use other_crate::helper;
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    #[test]
    fn test_use_in_normal_module_not_marked() {
        let source = r"
mod normal {
    use other_crate::helper;
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].context, EdgeContext::production());
    }

    #[test]
    fn test_cfg_test_on_use_item_marked() {
        let source = r"
#[cfg(test)]
use other_crate::test_helper;
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    #[test]
    fn test_nested_cfg_test_scope() {
        let source = r"
#[cfg(test)]
mod tests {
    mod inner {
        use other_crate::deep_helper;
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    #[test]
    fn test_cfg_all_test_feature_marked() {
        let source = r#"
#[cfg(all(test, feature = "hir"))]
mod hir_tests {
    use other_crate::helper;
}
"#;
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    #[test]
    fn test_cfg_all_feature_then_test_marked() {
        let source = r#"
#[cfg(all(feature = "hir", test))]
mod hir_tests {
    use other_crate::helper;
}
"#;
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    /// A `#[test]` fn is compiled only into the test harness, even in a module
    /// without `#[cfg(test)]`.
    #[test]
    fn test_use_in_test_fn_without_cfg_test_marked() {
        let source = r"
mod checks {
    #[test]
    fn check_examples() {
        use other_crate::helper;
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    /// `any(windows, test)` also builds outside the test harness, on Windows.
    #[test]
    fn test_cfg_any_with_non_test_branch_not_marked() {
        let source = r"
#[cfg(any(windows, test))]
mod platform {
    use other_crate::helper;
}
#[cfg(any(windows, test))]
fn platform_helper() {
    use other_crate::other_helper;
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].context, EdgeContext::production());
        assert_eq!(uses[1].context, EdgeContext::production());
    }

    #[test]
    fn test_cfg_any_with_only_test_branches_marked() {
        let source = r#"
#[cfg(any(test, all(test, feature = "hir")))]
mod hir_tests {
    use other_crate::helper;
}
"#;
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    #[test]
    fn test_cfg_test_on_fn_marked() {
        let source = r"
#[cfg(test)]
fn test_helper() {
    use other_crate::helper;
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    #[test]
    fn test_cfg_test_on_method_marked() {
        let source = r"
impl Widget {
    #[cfg(test)]
    fn fixture() {
        use other_crate::helper;
    }
    fn run() {
        use other_crate::engine;
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        let contexts: Vec<_> = uses.iter().map(|u| u.context.clone()).collect();
        assert_eq!(
            contexts,
            [
                EdgeContext::test(crate::model::TestKind::Unit),
                EdgeContext::production(),
            ]
        );
    }

    #[test]
    fn test_cfg_test_on_impl_block_marked() {
        let source = r"
#[cfg(test)]
impl Widget {
    fn fixture() {
        use other_crate::helper;
    }
}
impl Widget {
    fn run() {
        use other_crate::engine;
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        let contexts: Vec<_> = uses.iter().map(|u| u.context.clone()).collect();
        assert_eq!(
            contexts,
            [
                EdgeContext::test(crate::model::TestKind::Unit),
                EdgeContext::production(),
            ]
        );
    }
}

mod path_ref_tests {
    use super::*;

    #[test]
    fn test_collect_path_refs_expression() {
        let source = r"
fn main() {
    my_server::run();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "my_server::run"),
            "should collect my_server::run, found: {refs:?}"
        );
    }

    #[test]
    fn test_collect_path_refs_type_annotation() {
        let source = r"
fn main() {
    let _x: my_lib::Config = todo!();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "my_lib::Config"),
            "should collect my_lib::Config, found: {refs:?}"
        );
    }

    #[test]
    fn test_collect_path_refs_pattern() {
        let source = r"
fn main() {
    let x = 1;
    match x {
        _ if my_lib::check() => {}
        _ => {}
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "my_lib::check"),
            "should collect my_lib::check, found: {refs:?}"
        );
    }

    #[test]
    fn test_collect_path_refs_trait_bound() {
        let source = r"
fn process<T: my_lib::Trait>(_t: T) {}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "my_lib::Trait"),
            "should collect my_lib::Trait, found: {refs:?}"
        );
    }

    #[test]
    fn a_path_asks_the_meaning_of_its_position() {
        let source = r"
#[derive(my_lib::Marker)]
struct S(my_lib::Kind);
fn main() {
    my_lib::mac!(my_lib::arg);
    my_lib::run();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let meaning_of = |path: &str| refs.iter().find(|r| r.path == path).map(|r| r.meaning);
        assert_eq!(meaning_of("my_lib::Marker"), Some(Meaning::Macro));
        assert_eq!(meaning_of("my_lib::Kind"), Some(Meaning::Type));
        assert_eq!(meaning_of("my_lib::mac"), Some(Meaning::Macro));
        assert_eq!(meaning_of("my_lib::arg"), Some(Meaning::Any));
        assert_eq!(meaning_of("my_lib::run"), Some(Meaning::Value));
    }

    #[test]
    fn test_collect_path_refs_struct_literal() {
        let source = r"
fn main() {
    let _x = my_lib::Config { verbose: true };
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "my_lib::Config"),
            "should collect my_lib::Config from struct literal, found: {refs:?}"
        );
    }

    /// A bare name is an occurrence of what a `use` binds, or nothing. It never
    /// names a module, even one spelled the same.
    #[test]
    fn test_unbound_single_segment_is_no_dependency() {
        let source = r"
mod helper;
fn main() {
    let x = helper();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "helper"),
            "the bare name is collected: {refs:?}"
        );
        let ws = TestWorkspace::new()
            .package(
                "my_crate",
                &[("src/main.rs", source), ("src/helper.rs", "")],
            )
            .analyze();
        let deps = ws.deps("my_crate");
        assert!(deps.is_empty(), "no dependency from a bare name: {deps:?}");
    }

    #[rstest::rstest]
    #[case("crate::environment")]
    #[case("super::super")]
    fn test_collect_path_refs_ignores_visibility_scope(#[case] scope: &str) {
        let source = format!("pub struct Wrapper(pub(in {scope}) Inner);");
        let syntax = syn::parse_file(&source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        // A visibility scope grants access; it uses nothing from that module
        assert!(
            !refs.iter().any(|r| r.path == scope),
            "visibility scope should not be collected, found: {refs:?}"
        );
    }

    #[test]
    fn test_collect_path_refs_keeps_field_type_beside_visibility_scope() {
        let source = "pub struct Wrapper(pub(in crate::environment) other::Inner);";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "other::Inner"),
            "should collect other::Inner, found: {refs:?}"
        );
    }

    #[test]
    fn test_collect_path_refs_method_chain() {
        let source = r"
fn main() {
    my_lib::Config::default();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "my_lib::Config::default"),
            "should collect full path my_lib::Config::default, found: {refs:?}"
        );
    }

    #[test]
    fn test_collect_path_refs_multiple_in_file() {
        let source = r"
fn main() {
    my_server::run();
    let _cfg: my_lib::Config = todo!();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "my_server::run"),
            "should collect my_server::run, found: {refs:?}"
        );
        assert!(
            refs.iter().any(|r| r.path == "my_lib::Config"),
            "should collect my_lib::Config, found: {refs:?}"
        );
    }
}

mod path_ref_cfg_test_tests {
    use super::*;
    use crate::model::EdgeContext;

    #[test]
    fn test_path_ref_in_cfg_test_marked() {
        let source = r"
#[cfg(test)]
mod tests {
    fn check() {
        other_crate::module::helper();
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let matching: Vec<_> = refs
            .iter()
            .filter(|r| r.path == "other_crate::module::helper")
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(
            matching[0].context,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
    }

    #[test]
    fn test_path_ref_in_test_fn_without_cfg_test_marked() {
        let source = r"
mod checks {
    #[test]
    fn check_examples() -> support_crate::Result {
        support_crate::tester().run()
    }
}
fn after() {
    support_crate::other();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let context_of = |path: &str| {
            let matching: Vec<_> = refs.iter().filter(|r| r.path == path).collect();
            assert_eq!(matching.len(), 1, "{path}");
            matching[0].context.clone()
        };
        let test = EdgeContext::test(crate::model::TestKind::Unit);
        assert_eq!(context_of("support_crate::Result"), test);
        assert_eq!(context_of("support_crate::tester"), test);
        assert_eq!(
            context_of("support_crate::other"),
            EdgeContext::production()
        );
    }

    #[test]
    fn test_path_ref_in_cfg_test_fn_marked() {
        let source = r"
#[cfg(test)]
pub(crate) fn dir_name() -> io::Name {
    io::dir_name()
}
fn after() {
    io::other();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let context_of = |path: &str| {
            let matching: Vec<_> = refs.iter().filter(|r| r.path == path).collect();
            assert_eq!(matching.len(), 1, "{path}");
            matching[0].context.clone()
        };
        let test = EdgeContext::test(crate::model::TestKind::Unit);
        assert_eq!(context_of("io::Name"), test);
        assert_eq!(context_of("io::dir_name"), test);
        assert_eq!(context_of("io::other"), EdgeContext::production());
    }

    #[test]
    fn test_path_ref_in_cfg_test_method_marked() {
        let source = r"
impl Widget {
    #[cfg(test)]
    fn fixture() -> io::Name {
        io::dir_name()
    }
    fn run() {
        io::other();
    }
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let context_of = |path: &str| {
            let matching: Vec<_> = refs.iter().filter(|r| r.path == path).collect();
            assert_eq!(matching.len(), 1, "{path}");
            matching[0].context.clone()
        };
        let test = EdgeContext::test(crate::model::TestKind::Unit);
        assert_eq!(context_of("io::Name"), test);
        assert_eq!(context_of("io::dir_name"), test);
        assert_eq!(context_of("io::other"), EdgeContext::production());
    }

    #[test]
    fn test_path_ref_in_cfg_test_items_marked() {
        let source = r"
#[cfg(test)]
#[harness::gpu_test]
static TEST: harness::Params = harness::Params::new();
#[cfg(test)]
impl io::Reader for Widget {}
#[cfg(test)]
struct Fixture {
    name: io::Name,
}
#[cfg(test)]
const LIMIT: limits::Size = limits::DEFAULT;
static LIVE: io::Live = io::Live::new();
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let context_of = |path: &str| {
            let matching: Vec<_> = refs.iter().filter(|r| r.path == path).collect();
            assert_eq!(matching.len(), 1, "{path}");
            matching[0].context.clone()
        };
        let test = EdgeContext::test(crate::model::TestKind::Unit);
        assert_eq!(context_of("harness::gpu_test"), test);
        assert_eq!(context_of("harness::Params::new"), test);
        assert_eq!(context_of("io::Reader"), test);
        assert_eq!(context_of("io::Name"), test);
        assert_eq!(context_of("limits::Size"), test);
        assert_eq!(context_of("limits::DEFAULT"), test);
        assert_eq!(context_of("io::Live::new"), EdgeContext::production());
    }

    #[test]
    fn test_path_ref_in_normal_code_production() {
        let source = r"
fn main() {
    other_crate::module::run();
}
";
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let matching: Vec<_> = refs
            .iter()
            .filter(|r| r.path == "other_crate::module::run")
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].context, EdgeContext::production());
    }
}

mod region_tests {
    use super::*;

    /// A reference numbered by one walk is looked up against bindings numbered by
    /// the other. Disagreeing counts would mean disagreeing numbers.
    #[test]
    fn test_both_collectors_number_the_same_regions() {
        let file = syn::parse_file(
            "fn draw() { let _ = 1; }\n\
             mod inner { fn stats() { { use crate::metrics; metrics::count(); } } }\n\
             #[cfg(test)] mod tests { fn t() {} }",
        )
        .unwrap();
        let uses = collect_all_use_items(&file, EdgeContext::production());
        let refs = collect_all_path_refs(&file, EdgeContext::production());
        assert!(
            uses.regions.count() > 1,
            "the fixture is meant to nest regions"
        );
        assert_eq!(
            uses.regions.count(),
            refs.regions.count(),
            "both walks number the regions of the same file"
        );
    }
}
