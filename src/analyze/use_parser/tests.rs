use super::*;
use crate::analyze::reexports::ReExportMap;
use std::path::Path;
use std::sync::LazyLock;

static DEFAULT_WS: LazyLock<WorkspaceCrates> = LazyLock::new(WorkspaceCrates::default);
static DEFAULT_MP: LazyLock<ModulePathMap> = LazyLock::new(ModulePathMap::default);
static DEFAULT_EXPORTS: LazyLock<CrateExportMap> = LazyLock::new(CrateExportMap::default);
static DEFAULT_REEXPORT_MAP: LazyLock<ReExportMap> = LazyLock::new(ReExportMap::default);

struct ResolutionContextBuilder<'a> {
    current_crate: &'a str,
    workspace_crates: &'a WorkspaceCrates,
    source_file: &'a Path,
    all_module_paths: &'a ModulePathMap,
    crate_exports: &'a CrateExportMap,
    current_module_path: &'a str,
    reexport_map: &'a ReExportMap,
}

impl<'a> ResolutionContextBuilder<'a> {
    fn new(source_file: &'a Path) -> Self {
        Self {
            current_crate: "my_crate",
            workspace_crates: &DEFAULT_WS,
            source_file,
            all_module_paths: &DEFAULT_MP,
            crate_exports: &DEFAULT_EXPORTS,
            current_module_path: "",
            reexport_map: &DEFAULT_REEXPORT_MAP,
        }
    }

    fn current_crate(mut self, name: &'a str) -> Self {
        self.current_crate = name;
        self
    }

    fn workspace_crates(mut self, ws: &'a WorkspaceCrates) -> Self {
        self.workspace_crates = ws;
        self
    }

    fn module_paths(mut self, mp: &'a ModulePathMap) -> Self {
        self.all_module_paths = mp;
        self
    }

    fn crate_exports(mut self, exports: &'a CrateExportMap) -> Self {
        self.crate_exports = exports;
        self
    }

    fn current_module_path(mut self, path: &'a str) -> Self {
        self.current_module_path = path;
        self
    }

    fn reexport_map(mut self, map: &'a ReExportMap) -> Self {
        self.reexport_map = map;
        self
    }

    fn build(self) -> ResolutionContext<'a> {
        ResolutionContext {
            current_crate: self.current_crate,
            workspace_crates: self.workspace_crates,
            source_file: self.source_file,
            all_module_paths: self.all_module_paths,
            crate_exports: self.crate_exports,
            current_module_path: self.current_module_path,
            reexport_map: self.reexport_map,
        }
    }
}

fn parse_test_uses(source: &str) -> Vec<(syn::ItemUse, EdgeContext, usize)> {
    collect_all_use_items(&syn::parse_file(source).unwrap(), EdgeContext::production())
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
        let uses = parse_test_uses("use crate::graph::build;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert_eq!(dep.target_item, Some("build".to_string()));
    }

    #[test]
    fn test_process_use_statement_crate_local_module_only() {
        let uses = parse_test_uses("use crate::graph;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert!(dep.target_item.is_none());
        assert_eq!(dep.line, 1);
    }

    #[test]
    fn test_process_use_statement_workspace_crate() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::utils;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "other_crate");
        assert_eq!(dep.target_module, "utils");
    }

    #[test]
    fn test_process_use_statement_workspace_crate_with_hyphen() {
        let ws: WorkspaceCrates = ["my-lib".to_string()].into_iter().collect();
        let uses = parse_test_uses("use my_lib::feature;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .current_crate("app")
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "my_lib");
        assert_eq!(dep.target_module, "feature");
    }

    #[test]
    fn test_process_use_statement_relative_self_resolved() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["render".into(), "render::helper".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use self::helper;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/render/mod.rs"))
            .module_paths(&mp)
            .current_module_path("render")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1, "self:: should resolve: {deps:?}");
        assert_eq!(deps[0].target_crate, "my_crate");
        assert_eq!(deps[0].target_module, "render::helper");
    }

    #[test]
    fn test_process_use_statement_relative_super_resolved() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["parent".into(), "sub".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use super::parent;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/sub/mod.rs"))
            .module_paths(&mp)
            .current_module_path("sub")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1, "super:: should resolve: {deps:?}");
        assert_eq!(deps[0].target_crate, "my_crate");
        assert_eq!(deps[0].target_module, "parent");
    }

    #[test]
    fn test_process_use_statement_external_filtered() {
        let ws: WorkspaceCrates = ["my_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use serde::Serialize;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert!(deps.is_empty(), "external crate imports should be filtered");
    }

    #[test]
    fn test_process_use_statement_std_filtered() {
        let uses = parse_test_uses("use std::collections::HashMap;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert!(deps.is_empty(), "std imports should be filtered");
    }
}

mod longest_prefix_tests {
    use super::*;

    #[test]
    fn test_find_longest_prefix_submodule() {
        let paths: HashSet<String> =
            HashSet::from(["analyze".into(), "analyze::use_parser".into()]);
        let (prefix, len) =
            find_longest_module_prefix(&["analyze", "use_parser", "normalize"], &paths);
        assert_eq!(prefix, "analyze::use_parser");
        assert_eq!(len, 2);
    }

    #[test]
    fn test_find_longest_prefix_parent_only() {
        let paths: HashSet<String> = HashSet::from(["analyze".into()]);
        let (prefix, len) = find_longest_module_prefix(&["analyze", "SomeItem"], &paths);
        assert_eq!(prefix, "analyze");
        assert_eq!(len, 1);
    }

    #[test]
    fn test_find_longest_prefix_no_match() {
        let paths: HashSet<String> = HashSet::from(["analyze".into()]);
        let (prefix, len) = find_longest_module_prefix(&["unknown", "item"], &paths);
        assert_eq!(prefix, "unknown");
        assert_eq!(len, 1);
    }

    #[test]
    fn test_find_longest_prefix_single_segment() {
        let paths: HashSet<String> = HashSet::from(["graph".into()]);
        let (prefix, len) = find_longest_module_prefix(&["graph"], &paths);
        assert_eq!(prefix, "graph");
        assert_eq!(len, 1);
    }

    #[test]
    fn test_find_longest_prefix_empty_module_paths() {
        let paths: HashSet<String> = HashSet::new();
        let (prefix, len) = find_longest_module_prefix(&["analyze", "foo"], &paths);
        assert_eq!(prefix, "analyze");
        assert_eq!(len, 1);
    }
}

mod submodule_tests {
    use super::*;

    #[test]
    fn test_crate_local_submodule() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["analyze".into(), "analyze::use_parser".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::analyze::use_parser::normalize;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "analyze::use_parser");
        assert_eq!(dep.target_item, Some("normalize".to_string()));
    }

    #[test]
    fn test_workspace_import_submodule() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [(
            "other_crate".to_string(),
            HashSet::from(["foo".into(), "foo::bar".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use other_crate::foo::bar::Baz;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "foo::bar");
        assert_eq!(dep.target_item, Some("Baz".to_string()));
    }

    #[test]
    fn test_workspace_import_cross_crate_deep() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [(
            "other_crate".to_string(),
            HashSet::from(["sub".into(), "sub::deep".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use other_crate::sub::deep::Item;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "sub::deep");
        assert_eq!(dep.target_item, Some("Item".to_string()));
    }

    #[test]
    fn test_cross_crate_no_paths_fallback() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::foo::bar::Baz;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "foo");
        assert_eq!(dep.target_item, Some("bar".to_string()));
    }

    #[test]
    fn test_multi_symbol_with_submodule() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["analyze".into(), "analyze::use_parser".into()]),
        )]
        .into_iter()
        .collect();
        let uses =
            parse_test_uses("use crate::analyze::use_parser::{normalize, is_workspace_member};");
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 2, "should return 2 deps: {deps:?}");
        assert_eq!(deps[0].target_module, "analyze::use_parser");
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("normalize".to_string()))
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("is_workspace_member".to_string()))
        );
    }
}

mod parsing_tests {
    use super::*;

    #[test]
    fn test_parse_workspace_dependencies_mixed() {
        let source = r"
use crate::graph;
use other_crate::utils;
use serde::Serialize;
use std::collections::HashMap;
";
        let ws: WorkspaceCrates = ["my_crate".to_string(), "other_crate".to_string()]
            .into_iter()
            .collect();
        let uses = parse_test_uses(source);
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);

        assert_eq!(deps.len(), 2, "found: {deps:?}");
        assert!(
            deps.iter()
                .any(|d| d.target_crate == "my_crate" && d.target_module == "graph")
        );
        assert!(
            deps.iter()
                .any(|d| d.target_crate == "other_crate" && d.target_module == "utils")
        );
    }

    #[test]
    fn test_parse_workspace_dependencies_dedup_by_full_target() {
        let source = r"
use crate::graph::build;
use crate::graph::Node;
use crate::graph;
";
        let uses = parse_test_uses(source);
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);

        assert_eq!(deps.len(), 3, "should keep distinct symbols: {deps:?}");
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("build".to_string()))
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("Node".to_string()))
        );
        assert!(deps.iter().any(|d| d.target_item.is_none()));
    }

    #[test]
    fn test_process_use_multi_symbol() {
        let uses = parse_test_uses("use crate::graph::{Node, Edge};");
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 2, "should return 2 deps: {deps:?}");
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("Node".to_string()))
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("Edge".to_string()))
        );
    }

    #[test]
    fn test_process_use_glob() {
        let uses = parse_test_uses("use crate::analyze::*;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1, "glob should return 1 dep: {deps:?}");
        assert_eq!(deps[0].target_item, Some("*".to_string()));
        assert_eq!(deps[0].target_module, "analyze");
    }
}

mod integration_tests {
    use super::*;

    #[test]
    fn test_bare_module_pub_use_integration() {
        let source = r"
pub use cli::{Args, Cargo, run};
use crate::graph::build;
pub(crate) use model::Node;
use other_crate::utils;
use serde::Serialize;
";
        let ws: WorkspaceCrates = ["my_crate".to_string(), "other_crate".to_string()]
            .into_iter()
            .collect();
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["cli".into(), "graph".into(), "model".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses(source);
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);

        // Expected: 6 DependencyRefs
        // - cli::Args, cli::Cargo, cli::run (bare module, pub use multi)
        // - graph::build (crate:: prefix)
        // - model::Node (bare module, pub(crate))
        // - other_crate::utils (workspace crate)
        assert_eq!(deps.len(), 6, "expected 6 deps, got: {deps:?}");

        // Bare module multi-import (pub use)
        assert!(
            deps.iter().any(|d| d.target_crate == "my_crate"
                && d.target_module == "cli"
                && d.target_item == Some("Args".to_string())),
            "missing cli::Args"
        );
        assert!(
            deps.iter().any(|d| d.target_crate == "my_crate"
                && d.target_module == "cli"
                && d.target_item == Some("Cargo".to_string())),
            "missing cli::Cargo"
        );
        assert!(
            deps.iter().any(|d| d.target_crate == "my_crate"
                && d.target_module == "cli"
                && d.target_item == Some("run".to_string())),
            "missing cli::run"
        );

        // crate:: prefix
        assert!(
            deps.iter().any(|d| d.target_crate == "my_crate"
                && d.target_module == "graph"
                && d.target_item == Some("build".to_string())),
            "missing graph::build"
        );

        // Bare module pub(crate)
        assert!(
            deps.iter().any(|d| d.target_crate == "my_crate"
                && d.target_module == "model"
                && d.target_item == Some("Node".to_string())),
            "missing model::Node"
        );

        // Workspace crate
        assert!(
            deps.iter()
                .any(|d| d.target_crate == "other_crate" && d.target_module == "utils"),
            "missing other_crate::utils"
        );
    }
}

mod bare_module_tests {
    use super::*;

    #[test]
    fn test_bare_module_simple() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let dep = parse_bare_module_import(&ctx, "cli::Args", 1, &EdgeContext::production());
        let dep = dep.expect("should parse bare module import");
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "cli");
        assert_eq!(dep.target_item, Some("Args".to_string()));
    }

    #[test]
    fn test_bare_module_no_match() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let dep = parse_bare_module_import(&ctx, "serde::Serialize", 1, &EdgeContext::production());
        assert!(dep.is_none(), "external crate should not match");
    }

    #[test]
    fn test_bare_module_deep_path() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["analyze".into(), "analyze::use_parser".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let dep = parse_bare_module_import(
            &ctx,
            "analyze::use_parser::normalize",
            1,
            &EdgeContext::production(),
        );
        let dep = dep.expect("should parse deep bare module import");
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "analyze::use_parser");
        assert_eq!(dep.target_item, Some("normalize".to_string()));
    }

    #[test]
    fn test_bare_module_module_only() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let dep = parse_bare_module_import(&ctx, "cli", 1, &EdgeContext::production());
        let dep = dep.expect("should parse module-only bare import");
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "cli");
        assert!(dep.target_item.is_none());
    }

    #[test]
    fn test_bare_module_via_parse_workspace_dependencies() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let uses = parse_test_uses("use cli::Args;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "cli");
        assert_eq!(dep.target_item, Some("Args".to_string()));
    }

    #[test]
    fn test_bare_module_pub_use_via_parse_workspace_dependencies() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let uses = parse_test_uses("pub(crate) use cli::Args;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "cli");
        assert_eq!(dep.target_item, Some("Args".to_string()));
    }

    #[test]
    fn test_bare_module_multi_import() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let uses = parse_test_uses("use cli::{Args, Cargo, run};");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 3, "should return 3 deps: {deps:?}");
        assert!(deps.iter().all(|d| d.target_crate == "my_crate"));
        assert!(deps.iter().all(|d| d.target_module == "cli"));
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("Args".to_string()))
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("Cargo".to_string()))
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("run".to_string()))
        );
    }

    #[test]
    fn test_bare_module_glob_import() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let uses = parse_test_uses("use cli::*;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1, "glob should return 1 dep: {deps:?}");
        assert_eq!(deps[0].target_crate, "my_crate");
        assert_eq!(deps[0].target_module, "cli");
        assert_eq!(deps[0].target_item, Some("*".to_string()));
    }

    #[test]
    fn test_bare_module_child_resolution() {
        // `use css::render_styles` in render/mod.rs should resolve to render::css
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["render".into(), "render::css".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/render/mod.rs"))
            .module_paths(&mp)
            .current_module_path("render")
            .build();
        let dep =
            parse_bare_module_import(&ctx, "css::render_styles", 1, &EdgeContext::production());
        let dep = dep.expect("should resolve child module");
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "render::css");
        assert_eq!(dep.target_item, Some("render_styles".to_string()));
    }

    #[test]
    fn test_bare_module_child_multi_import() {
        // Group import `use elements::{A, B}` in render/mod.rs
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["render".into(), "render::elements".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use elements::{LinkTag, ScriptTag};");
        let ctx = ResolutionContextBuilder::new(Path::new("src/render/mod.rs"))
            .module_paths(&mp)
            .current_module_path("render")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 2, "should return 2 deps: {deps:?}");
        assert!(deps.iter().all(|d| d.target_module == "render::elements"));
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("LinkTag".to_string()))
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("ScriptTag".to_string()))
        );
    }

    #[test]
    fn test_bare_module_root_still_works() {
        // current_module_path="" → existing behavior unchanged
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let dep = parse_bare_module_import(&ctx, "cli::Args", 1, &EdgeContext::production());
        let dep = dep.expect("should parse bare module from root");
        assert_eq!(dep.target_module, "cli");
        assert_eq!(dep.target_item, Some("Args".to_string()));
    }

    #[test]
    fn test_bare_module_deeply_nested() {
        // `use sub::Item` in a::b → resolves to a::b::sub
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["a".into(), "a::b".into(), "a::b::sub".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/a/b/mod.rs"))
            .module_paths(&mp)
            .current_module_path("a::b")
            .build();
        let dep = parse_bare_module_import(&ctx, "sub::Item", 1, &EdgeContext::production());
        let dep = dep.expect("should resolve deeply nested child");
        assert_eq!(dep.target_module, "a::b::sub");
        assert_eq!(dep.target_item, Some("Item".to_string()));
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

    #[test]
    fn test_entry_point_export_detected() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
            .into_iter()
            .collect();
        let exports: CrateExportMap = [(
            "other_crate".to_string(),
            HashSet::from(["MyStruct".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use other_crate::MyStruct;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .crate_exports(&exports)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "other_crate");
        assert_eq!(dep.target_module, "");
        assert_eq!(dep.target_item, Some("MyStruct".to_string()));
    }

    #[test]
    fn test_non_export_stays_fallback() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
            .into_iter()
            .collect();
        let exports: CrateExportMap = [("other_crate".to_string(), HashSet::new())]
            .into_iter()
            .collect();
        let uses = parse_test_uses("use other_crate::Unknown;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .crate_exports(&exports)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "Unknown");
        assert!(dep.target_item.is_none());
    }

    #[test]
    fn test_real_module_not_affected() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
            .into_iter()
            .collect();
        let exports: CrateExportMap =
            [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
                .into_iter()
                .collect();
        let uses = parse_test_uses("use other_crate::sub_mod::Foo;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .crate_exports(&exports)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "sub_mod");
        assert_eq!(dep.target_item, Some("Foo".to_string()));
    }

    #[test]
    fn test_entry_point_multi_import() {
        // Without module path info, `use other_crate::{Foo, Bar}` resolves each
        // symbol as a module-level fallback (target_module="Foo", no target_item).
        // The dependency on other_crate is still correctly detected.
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::{Foo, Bar};");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 2, "should return 2 deps: {deps:?}");
        assert!(deps.iter().all(|d| d.target_crate == "other_crate"));
        assert!(deps.iter().any(|d| d.target_module == "Foo"));
        assert!(deps.iter().any(|d| d.target_module == "Bar"));
    }

    #[test]
    fn test_entry_point_glob_import() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::*;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1, "glob should return 1 dep: {deps:?}");
        assert_eq!(deps[0].target_module, "");
        assert_eq!(deps[0].target_item, Some("*".to_string()));
    }
}

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
        assert_eq!(uses[0].1, EdgeContext::test(crate::model::TestKind::Unit));
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
        assert_eq!(uses[0].1, EdgeContext::production());
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
        assert_eq!(uses[0].1, EdgeContext::test(crate::model::TestKind::Unit));
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
        assert_eq!(uses[0].1, EdgeContext::test(crate::model::TestKind::Unit));
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
        assert_eq!(uses[0].1, EdgeContext::test(crate::model::TestKind::Unit));
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
        assert_eq!(uses[0].1, EdgeContext::test(crate::model::TestKind::Unit));
    }

    /// Known limitation: `#[cfg(test)]` on `fn` items is NOT detected —
    /// only `mod`-level `#[cfg(test)]` propagates. This is acceptable because
    /// the dominant pattern is `#[cfg(test)] mod tests { ... }`.
    #[test]
    fn test_cfg_test_on_fn_not_detected() {
        let source = r"
#[cfg(test)]
fn test_helper() {
    use other_crate::helper;
}
";
        let syntax = syn::parse_file(source).unwrap();
        let uses = collect_all_use_items(&syntax, EdgeContext::production());
        assert_eq!(uses.len(), 1);
        // fn-level cfg(test) is NOT propagated — use is tagged Production
        assert_eq!(uses[0].1, EdgeContext::production());
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
            refs.iter().any(|(p, _, _, _)| p == "my_server::run"),
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
            refs.iter().any(|(p, _, _, _)| p == "my_lib::Config"),
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
            refs.iter().any(|(p, _, _, _)| p == "my_lib::check"),
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
            refs.iter().any(|(p, _, _, _)| p == "my_lib::Trait"),
            "should collect my_lib::Trait, found: {refs:?}"
        );
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
            refs.iter().any(|(p, _, _, _)| p == "my_lib::Config"),
            "should collect my_lib::Config from struct literal, found: {refs:?}"
        );
    }

    #[test]
    fn test_collect_path_refs_ignores_single_segment() {
        let source = r#"
fn main() {
    println!("hello");
    let x = String::new();
}
"#;
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        // "println" is a single segment → not collected
        assert!(
            !refs.iter().any(|(p, _, _, _)| p == "println"),
            "single-segment paths should not be collected, found: {refs:?}"
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
            refs.iter()
                .any(|(p, _, _, _)| p == "my_lib::Config::default"),
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
            refs.iter().any(|(p, _, _, _)| p == "my_server::run"),
            "should collect my_server::run, found: {refs:?}"
        );
        assert!(
            refs.iter().any(|(p, _, _, _)| p == "my_lib::Config"),
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
            .filter(|(p, _, _, _)| p == "other_crate::module::helper")
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(
            matching[0].2,
            EdgeContext::test(crate::model::TestKind::Unit)
        );
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
            .filter(|(p, _, _, _)| p == "other_crate::module::run")
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].2, EdgeContext::production());
    }
}

mod path_ref_resolution_tests {
    use super::*;
    use crate::model::EdgeContext;

    #[test]
    fn test_parse_path_refs_workspace_crate() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();
        let paths = vec![(
            "other_crate::module::item".to_string(),
            5,
            EdgeContext::production(),
            0,
        )];
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(
            deps.len(),
            1,
            "should resolve workspace crate path: {deps:?}"
        );
        assert_eq!(deps[0].target_crate, "other_crate");
        assert_eq!(deps[0].target_module, "module");
        assert_eq!(deps[0].target_item, Some("item".to_string()));
        assert_eq!(deps[0].line, 5);
    }

    #[test]
    fn test_parse_path_refs_crate_local() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();
        let paths = vec![(
            "crate::module::item".to_string(),
            3,
            EdgeContext::production(),
            0,
        )];
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(deps.len(), 1, "should resolve crate-local path: {deps:?}");
        assert_eq!(deps[0].target_crate, "my_crate");
        assert_eq!(deps[0].target_module, "module");
        assert_eq!(deps[0].target_item, Some("item".to_string()));
    }

    #[test]
    fn test_parse_path_refs_bare_module() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["cli".into()]))]
            .into_iter()
            .collect();
        let paths = vec![("cli::Args".to_string(), 1, EdgeContext::production(), 0)];
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(deps.len(), 1, "should resolve bare module path: {deps:?}");
        assert_eq!(deps[0].target_crate, "my_crate");
        assert_eq!(deps[0].target_module, "cli");
        assert_eq!(deps[0].target_item, Some("Args".to_string()));
    }

    #[test]
    fn test_parse_path_refs_unknown_skipped() {
        let paths = vec![
            ("std::io::Read".to_string(), 1, EdgeContext::production(), 0),
            (
                "anyhow::Result".to_string(),
                2,
                EdgeContext::production(),
                0,
            ),
            (
                "serde::Serialize".to_string(),
                3,
                EdgeContext::production(),
                0,
            ),
        ];
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert!(deps.is_empty(), "unknown paths should be skipped: {deps:?}");
    }

    #[test]
    fn test_parse_path_refs_entry_point() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let exports: CrateExportMap = [(
            "other_crate".to_string(),
            HashSet::from(["MyStruct".into()]),
        )]
        .into_iter()
        .collect();
        let paths = vec![(
            "other_crate::MyStruct".to_string(),
            7,
            EdgeContext::production(),
            0,
        )];
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .workspace_crates(&ws)
            .crate_exports(&exports)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(deps.len(), 1, "should resolve entry-point path: {deps:?}");
        assert_eq!(deps[0].target_crate, "other_crate");
        assert_eq!(deps[0].target_module, "");
        assert_eq!(deps[0].target_item, Some("MyStruct".to_string()));
    }

    #[test]
    fn test_parse_path_refs_dedup() {
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();
        let paths = vec![
            (
                "other_crate::module::item".to_string(),
                5,
                EdgeContext::production(),
                0,
            ),
            (
                "other_crate::module::item".to_string(),
                10,
                EdgeContext::production(),
                0,
            ),
        ];
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(deps.len(), 1, "duplicate paths should be deduped: {deps:?}");
    }
}

mod relative_path_tests {
    use super::*;

    #[test]
    fn test_super_basic() {
        // super::foo from module "a::b", depth 0 → "a::foo"
        let result = resolve_relative_path("super::foo", "a::b", 0);
        assert_eq!(result, Some("a::foo".to_string()));
    }

    #[test]
    fn test_super_from_root() {
        // super::foo from crate root (empty path) → None (no parent)
        let result = resolve_relative_path("super::foo", "", 0);
        assert_eq!(result, None);
    }

    #[test]
    fn test_super_super() {
        // super::super::foo from "a::b::c" → "a::foo"
        let result = resolve_relative_path("super::super::foo", "a::b::c", 0);
        assert_eq!(result, Some("a::foo".to_string()));
    }

    #[test]
    fn test_super_too_many() {
        // super::super::foo from "a" → None (can't go above root)
        let result = resolve_relative_path("super::super::foo", "a", 0);
        assert_eq!(result, None);
    }

    #[test]
    fn test_self_basic() {
        // self::child from "render", depth 0 → "render::child"
        let result = resolve_relative_path("self::child", "render", 0);
        assert_eq!(result, Some("render::child".to_string()));
    }

    #[test]
    fn test_self_from_root() {
        // self::foo from crate root → "foo"
        let result = resolve_relative_path("self::foo", "", 0);
        assert_eq!(result, Some("foo".to_string()));
    }

    #[test]
    fn test_super_inside_inline_mod_ignored() {
        // `use super::*` inside mod tests {} (depth=1) → None (self-reference)
        let result = resolve_relative_path("super::*", "analyze::filtering", 1);
        assert_eq!(result, None);
    }

    #[test]
    fn test_super_super_from_inline_mod() {
        // `use super::super::sibling` inside mod tests {} (depth=1)
        // first super exits inline mod, second strips from current_module_path
        let result = resolve_relative_path("super::super::sibling", "analyze::filtering", 1);
        assert_eq!(result, Some("analyze::sibling".to_string()));
    }

    #[test]
    fn test_self_inside_inline_mod_ignored() {
        // `use self::helper` inside mod tests {} (depth=1) → None (inline scope)
        let result = resolve_relative_path("self::helper", "analyze::filtering", 1);
        assert_eq!(result, None);
    }
}

mod relative_import_e2e_tests {
    use super::*;

    #[test]
    fn test_super_filtering_from_workspace() {
        // Realistic: `use super::filtering::DepInfo` from analyze::workspace
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "analyze".into(),
                "analyze::filtering".into(),
                "analyze::workspace".into(),
                "analyze::use_parser".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use super::filtering::DepInfo;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/analyze/workspace.rs"))
            .module_paths(&mp)
            .current_module_path("analyze::workspace")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1, "super::filtering should resolve: {deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "analyze::filtering");
        assert_eq!(dep.target_item, Some("DepInfo".to_string()));
    }

    #[test]
    fn test_super_use_parser_from_syn_walker() {
        // `use super::use_parser::{A, B}` from analyze::syn_walker
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "analyze".into(),
                "analyze::syn_walker".into(),
                "analyze::use_parser".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses(
            "use super::use_parser::{parse_workspace_dependencies, collect_all_use_items};",
        );
        let ctx = ResolutionContextBuilder::new(Path::new("src/analyze/syn_walker.rs"))
            .module_paths(&mp)
            .current_module_path("analyze::syn_walker")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(
            deps.len(),
            2,
            "super::use_parser multi-import should resolve: {deps:?}"
        );
        assert!(deps.iter().all(|d| d.target_crate == "my_crate"));
        assert!(
            deps.iter()
                .all(|d| d.target_module == "analyze::use_parser")
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("parse_workspace_dependencies".to_string()))
        );
        assert!(
            deps.iter()
                .any(|d| d.target_item == Some("collect_all_use_items".to_string()))
        );
    }

    #[test]
    fn test_super_from_crate_root_ignored() {
        // super:: from crate root should produce no deps
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["some_mod".into()]))]
            .into_iter()
            .collect();
        let uses = parse_test_uses("use super::some_mod;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert!(
            deps.is_empty(),
            "super:: from root should produce no deps: {deps:?}"
        );
    }

    #[test]
    fn test_self_child_module() {
        // `use self::child::Item` from render
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["render".into(), "render::child".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use self::child::Item;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/render/mod.rs"))
            .module_paths(&mp)
            .current_module_path("render")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1, "self::child should resolve: {deps:?}");
        assert_eq!(deps[0].target_crate, "my_crate");
        assert_eq!(deps[0].target_module, "render::child");
        assert_eq!(deps[0].target_item, Some("Item".to_string()));
    }
}

mod inline_module_depth_tests {
    use super::*;

    #[test]
    fn test_super_star_in_cfg_test_mod_no_upward_edge() {
        // `#[cfg(test)] mod tests { use super::*; }` in filtering.rs
        // must NOT create an edge to the parent module
        let source = r"
fn some_fn() {}

#[cfg(test)]
mod tests {
    use super::*;
}
";
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["analyze".into(), "analyze::filtering".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses(source);
        let ctx = ResolutionContextBuilder::new(Path::new("src/analyze/filtering.rs"))
            .module_paths(&mp)
            .current_module_path("analyze::filtering")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert!(
            deps.is_empty(),
            "super::* in mod tests should not create upward edge: {deps:?}"
        );
    }

    #[test]
    fn test_super_super_from_inline_mod_creates_real_edge() {
        // `mod tests { use super::super::sibling::Item; }` should create
        // a real edge because it goes above current_module_path
        let source = r"
#[cfg(test)]
mod tests {
    use super::super::sibling::Item;
}
";
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "analyze".into(),
                "analyze::filtering".into(),
                "analyze::sibling".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses(source);
        let ctx = ResolutionContextBuilder::new(Path::new("src/analyze/filtering.rs"))
            .module_paths(&mp)
            .current_module_path("analyze::filtering")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(
            deps.len(),
            1,
            "super::super from inline mod should create real edge: {deps:?}"
        );
        assert_eq!(deps[0].target_module, "analyze::sibling");
        assert_eq!(deps[0].target_item, Some("Item".to_string()));
    }

    #[test]
    fn test_collect_use_items_tracks_inline_depth() {
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
        // top-level use: depth 0
        assert_eq!(uses[0].2, 0, "top-level use should have depth 0");
        // use inside mod inner: depth 1
        assert_eq!(uses[1].2, 1, "use in mod inner should have depth 1");
        // use inside mod inner::deep: depth 2
        assert_eq!(uses[2].2, 2, "use in mod deep should have depth 2");
    }
}

mod context_aware_dedup_tests {
    use super::*;
    use crate::model::{EdgeContext, TestKind};

    #[test]
    fn test_same_target_different_context_not_deduped_use() {
        // Production and Test dep on same symbol must both survive dedup
        let source = "use crate::graph::Node;";
        let syntax = syn::parse_file(source).unwrap();
        let item = syntax
            .items
            .into_iter()
            .find_map(|i| match i {
                syn::Item::Use(u) => Some(u),
                _ => None,
            })
            .unwrap();
        let uses = vec![
            (item.clone(), EdgeContext::production(), 0),
            (item, EdgeContext::test(TestKind::Unit), 0),
        ];
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(
            deps.len(),
            2,
            "prod + test on same target must not be deduped: {deps:?}"
        );
        assert!(deps.iter().any(|d| d.context == EdgeContext::production()));
        assert!(
            deps.iter()
                .any(|d| d.context == EdgeContext::test(TestKind::Unit))
        );
    }

    #[test]
    fn test_same_target_different_context_not_deduped_path_ref() {
        // Production and Test dep on same path ref must both survive dedup
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();
        let paths = vec![
            (
                "other_crate::module::item".to_string(),
                5,
                EdgeContext::production(),
                0,
            ),
            (
                "other_crate::module::item".to_string(),
                10,
                EdgeContext::test(TestKind::Unit),
                0,
            ),
        ];
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(
            deps.len(),
            2,
            "prod + test on same target must not be deduped: {deps:?}"
        );
        assert!(deps.iter().any(|d| d.context == EdgeContext::production()));
        assert!(
            deps.iter()
                .any(|d| d.context == EdgeContext::test(TestKind::Unit))
        );
    }

    #[test]
    fn test_same_target_same_context_still_deduped() {
        // Two Production deps on same symbol should still be deduped
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();
        let paths = vec![
            (
                "other_crate::module::item".to_string(),
                5,
                EdgeContext::production(),
                0,
            ),
            (
                "other_crate::module::item".to_string(),
                10,
                EdgeContext::production(),
                0,
            ),
        ];
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(
            deps.len(),
            1,
            "same context same target should still dedup: {deps:?}"
        );
    }

    #[test]
    fn test_dedup_merges_features_path_ref() {
        // Same target+kind with different features → one dep with merged features
        let ws: WorkspaceCrates = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();
        let paths = vec![
            (
                "other_crate::module::item".to_string(),
                5,
                EdgeContext {
                    kind: DependencyKind::Production,
                    features: vec!["feat-a".into()],
                },
                0,
            ),
            (
                "other_crate::module::item".to_string(),
                10,
                EdgeContext {
                    kind: DependencyKind::Production,
                    features: vec!["feat-b".into()],
                },
                0,
            ),
        ];
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(
            deps.len(),
            1,
            "same target+kind should dedup to one: {deps:?}"
        );
        let mut features = deps[0].context.features.clone();
        features.sort();
        assert_eq!(
            features,
            vec!["feat-a".to_string(), "feat-b".to_string()],
            "features should be merged: {features:?}"
        );
    }

    #[test]
    fn test_dedup_merges_features_use() {
        // Same target+kind via use-items with different features → merged
        let source = "use crate::graph::Node;";
        let syntax = syn::parse_file(source).unwrap();
        let item = syntax
            .items
            .into_iter()
            .find_map(|i| match i {
                syn::Item::Use(u) => Some(u),
                _ => None,
            })
            .unwrap();
        let uses = vec![
            (
                item.clone(),
                EdgeContext {
                    kind: DependencyKind::Production,
                    features: vec!["feat-a".into()],
                },
                0,
            ),
            (
                item,
                EdgeContext {
                    kind: DependencyKind::Production,
                    features: vec!["feat-b".into()],
                },
                0,
            ),
        ];
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(
            deps.len(),
            1,
            "same target+kind should dedup to one: {deps:?}"
        );
        let mut features = deps[0].context.features.clone();
        features.sort();
        assert_eq!(
            features,
            vec!["feat-a".to_string(), "feat-b".to_string()],
            "features should be merged: {features:?}"
        );
    }
}

mod reexport_resolution_tests {
    use super::*;
    use crate::analyze::reexports::{ModuleExportInfo, ReExportMap, ReExportTarget};

    #[test]
    fn without_reexport_map_unchanged() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::parent::Item;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target_module, "parent");
        assert_eq!(deps[0].target_item, Some("Item".to_string()));
    }

    #[test]
    fn super_import_resolved_via_reexport_map() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into(), "consumer".into()]),
        )]
        .into_iter()
        .collect();

        let mut parent_info = ModuleExportInfo::default();
        parent_info.explicit_reexports.insert(
            "Item".to_string(),
            ReExportTarget {
                module: "parent::sibling".to_string(),
                original_name: "Item".to_string(),
            },
        );
        let mut sibling_info = ModuleExportInfo::default();
        sibling_info.definitions.insert("Item".to_string());
        let mut crate_exports = HashMap::new();
        crate_exports.insert("parent".to_string(), parent_info);
        crate_exports.insert("parent::sibling".to_string(), sibling_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let uses = parse_test_uses("use super::parent::Item;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .current_module_path("consumer")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        assert_eq!(
            deps[0].target_module, "parent::sibling",
            "should resolve through re-export to sibling"
        );
    }

    #[test]
    fn crate_path_resolved_via_reexport_map() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["parent".into(), "parent::child".into()]),
        )]
        .into_iter()
        .collect();

        let mut parent_info = ModuleExportInfo::default();
        parent_info.explicit_reexports.insert(
            "Config".to_string(),
            ReExportTarget {
                module: "parent::child".to_string(),
                original_name: "Config".to_string(),
            },
        );
        let mut child_info = ModuleExportInfo::default();
        child_info.definitions.insert("Config".to_string());
        let mut crate_exports = HashMap::new();
        crate_exports.insert("parent".to_string(), parent_info);
        crate_exports.insert("parent::child".to_string(), child_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let uses = parse_test_uses("use crate::parent::Config;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx);
        assert_eq!(deps.len(), 1);
        assert_eq!(
            deps[0].target_module, "parent::child",
            "should resolve crate:: path through re-export"
        );
    }

    #[test]
    fn path_ref_resolved_via_reexport_map() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["parent".into(), "parent::child".into()]),
        )]
        .into_iter()
        .collect();

        let mut parent_info = ModuleExportInfo::default();
        parent_info.explicit_reexports.insert(
            "Config".to_string(),
            ReExportTarget {
                module: "parent::child".to_string(),
                original_name: "Config".to_string(),
            },
        );
        let mut child_info = ModuleExportInfo::default();
        child_info.definitions.insert("Config".to_string());
        let mut crate_exports = HashMap::new();
        crate_exports.insert("parent".to_string(), parent_info);
        crate_exports.insert("parent::child".to_string(), child_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let paths = vec![(
            "crate::parent::Config".to_string(),
            5,
            EdgeContext::production(),
            0,
        )];
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .reexport_map(&map)
            .build();
        let deps = parse_path_ref_dependencies(&paths, &ctx);
        assert_eq!(deps.len(), 1);
        assert_eq!(
            deps[0].target_module, "parent::child",
            "path refs should also resolve through re-export"
        );
    }
}
