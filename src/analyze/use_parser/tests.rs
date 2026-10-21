use super::ReExportMap;
use super::*;
use crate::model::{DefKind, Definition};
use std::path::Path;
use std::sync::LazyLock;

static DEFAULT_WS: LazyLock<WorkspacePackages> = LazyLock::new(WorkspacePackages::default);
static DEFAULT_MP: LazyLock<ModulePathMap> = LazyLock::new(ModulePathMap::default);
static DEFAULT_EXPORTS: LazyLock<PackageExportMap> = LazyLock::new(PackageExportMap::default);
static DEFAULT_REEXPORT_MAP: LazyLock<ReExportMap> = LazyLock::new(ReExportMap::default);
static DEFAULT_EXT_NAMES: LazyLock<HashMap<String, String>> = LazyLock::new(HashMap::new);

struct ResolutionContextBuilder<'a> {
    current_crate: &'a str,
    workspace_crates: &'a WorkspacePackages,
    source_file: &'a Path,
    all_module_paths: &'a ModulePathMap,
    crate_exports: &'a PackageExportMap,
    current_module_path: &'a str,
    reexport_map: &'a ReExportMap,
    external_crate_names: &'a HashMap<String, String>,
    extern_crates: &'a HashMap<String, String>,
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
            external_crate_names: &DEFAULT_EXT_NAMES,
            extern_crates: &DEFAULT_EXT_NAMES,
        }
    }

    fn current_crate(mut self, name: &'a str) -> Self {
        self.current_crate = name;
        self
    }

    fn workspace_crates(mut self, ws: &'a WorkspacePackages) -> Self {
        self.workspace_crates = ws;
        self
    }

    fn module_paths(mut self, mp: &'a ModulePathMap) -> Self {
        self.all_module_paths = mp;
        self
    }

    fn crate_exports(mut self, exports: &'a PackageExportMap) -> Self {
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

    fn external_crate_names(mut self, names: &'a HashMap<String, String>) -> Self {
        self.external_crate_names = names;
        self
    }

    fn extern_crates(mut self, aliases: &'a HashMap<String, String>) -> Self {
        self.extern_crates = aliases;
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
            external_crate_names: self.external_crate_names,
            extern_crates: self.extern_crates,
            passed_on: None,
        }
    }
}

fn parse_test_uses(source: &str) -> CollectedUses {
    collect_all_use_items(&syn::parse_file(source).unwrap(), EdgeContext::production())
}

fn bindings_of(source: &str, ctx: &ResolutionContext) -> FileBindings {
    let syntax = syn::parse_file(source).unwrap();
    let uses = collect_all_use_items(&syntax, EdgeContext::production());
    FileBindings::of(&syntax, &uses, ctx)
}

/// Path references standing in the file's own region, from the tuple form the
/// fixtures write them in.
fn root_path_refs(paths: Vec<(String, usize, EdgeContext, usize)>) -> CollectedPathRefs {
    CollectedPathRefs {
        refs: paths
            .into_iter()
            .map(|(path, line, context, inline_depth)| PathRef {
                path,
                line,
                context,
                inline_depth,
                region: BindingRegions::ROOT,
                position: PathPosition::Value,
                in_macro: false,
                local: None,
            })
            .collect(),
        regions: BindingRegions::default(),
    }
}

/// Resolve path references in the file's own region against a file that binds
/// nothing.
fn resolve_root_refs(
    paths: Vec<(String, usize, EdgeContext, usize)>,
    ctx: &ResolutionContext,
) -> Vec<DependencyRef> {
    parse_path_ref_dependencies(&root_path_refs(paths), ctx, &FileBindings::default())
}

/// `use` items in the file's own region, from the tuple form the fixtures write
/// them in.
fn root_uses(uses: Vec<(syn::ItemUse, EdgeContext, usize)>) -> CollectedUses {
    CollectedUses {
        items: uses
            .into_iter()
            .map(|(item, context, inline_depth)| CollectedUse {
                item,
                context,
                inline_depth,
                region: BindingRegions::ROOT,
            })
            .collect(),
        regions: BindingRegions::default(),
    }
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
        let uses = parse_test_uses("use crate::graph::build;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert_eq!(dep.target_item, Some("build".to_string()));
    }

    #[test]
    fn test_process_use_statement_crate_local_module_only() {
        let uses = parse_test_uses("use crate::graph;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert!(dep.target_item.is_none());
        assert_eq!(dep.line, 1);
    }

    #[test]
    fn test_process_use_statement_workspace_crate() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::utils;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_package, "other_crate");
        assert_eq!(dep.target_module, "utils");
    }

    #[test]
    fn test_process_use_statement_workspace_crate_with_hyphen() {
        let ws: WorkspacePackages = ["my-lib".to_string()].into_iter().collect();
        let uses = parse_test_uses("use my_lib::feature;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .current_crate("app")
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_lib");
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "self:: should resolve: {deps:?}");
        assert_eq!(deps[0].target_package, "my_crate");
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "super:: should resolve: {deps:?}");
        assert_eq!(deps[0].target_package, "my_crate");
        assert_eq!(deps[0].target_module, "parent");
    }

    #[test]
    fn test_process_use_statement_external_filtered() {
        let ws: WorkspacePackages = ["my_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use serde::Serialize;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert!(deps.is_empty(), "external crate imports should be filtered");
    }

    #[test]
    fn test_process_use_statement_std_filtered() {
        let uses = parse_test_uses("use std::collections::HashMap;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "analyze::use_parser");
        assert_eq!(dep.target_item, Some("normalize".to_string()));
    }

    #[test]
    fn test_workspace_import_submodule() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "foo::bar");
        assert_eq!(dep.target_item, Some("Baz".to_string()));
    }

    #[test]
    fn test_workspace_import_cross_crate_deep() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "sub::deep");
        assert_eq!(dep.target_item, Some("Item".to_string()));
    }

    #[test]
    fn test_workspace_import_through_root_glob_reexport() {
        // other_crate's root: `mod ty; pub use ty::*;`, ty.rs defines `Type`.
        // The root file names `Type` nowhere, only the glob brings it in.
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["ty".into()]))]
            .into_iter()
            .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.glob_sources.push("ty".to_string());
        let mut ty_info = ModuleExportInfo::default();
        ty_info.definitions.insert(
            "Type".to_string(),
            Definition {
                kind: DefKind::Enum,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "other_crate".to_string(),
            HashMap::from([(String::new(), root_info), ("ty".to_string(), ty_info)]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use other_crate::Type;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "other_crate");
        assert_eq!(dep.target_module, "ty");
        assert_eq!(dep.target_item, Some("Type".to_string()));
    }

    #[test]
    fn test_workspace_import_through_module_from_root_glob() {
        // other_crate's root: `mod errors; pub use errors::*;`, and errors.rs
        // holds `pub mod shell_error;`. The glob brings the module `shell_error`
        // into the root, and the path continues through it.
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [(
            "other_crate".to_string(),
            HashSet::from(["errors".into(), "errors::shell_error".into()]),
        )]
        .into_iter()
        .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.glob_sources.push("errors".to_string());
        let mut shell_error_info = ModuleExportInfo::default();
        shell_error_info.definitions.insert(
            "ShellError".to_string(),
            Definition {
                kind: DefKind::Enum,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "other_crate".to_string(),
            HashMap::from([
                (String::new(), root_info),
                ("errors".to_string(), ModuleExportInfo::default()),
                ("errors::shell_error".to_string(), shell_error_info),
            ]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use other_crate::shell_error::ShellError;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "other_crate");
        assert_eq!(dep.target_module, "errors::shell_error");
        assert_eq!(dep.target_item, Some("ShellError".to_string()));
    }

    #[test]
    fn test_workspace_import_through_module_from_named_root_reexport() {
        // other_crate's root: `mod backend; mod dispatch; pub use backend::custom;`,
        // and backend/custom.rs re-exports `pub use crate::dispatch::Interface;`.
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [(
            "other_crate".to_string(),
            HashSet::from([
                "backend".into(),
                "backend::custom".into(),
                "dispatch".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.explicit_reexports.insert(
            "custom".to_string(),
            ReExportTarget {
                crate_name: "other_crate".to_string(),
                module: "backend".to_string(),
                original_name: "custom".to_string(),
            },
        );
        let mut custom_info = ModuleExportInfo::default();
        custom_info.explicit_reexports.insert(
            "Interface".to_string(),
            ReExportTarget {
                crate_name: "other_crate".to_string(),
                module: "dispatch".to_string(),
                original_name: "Interface".to_string(),
            },
        );
        let mut dispatch_info = ModuleExportInfo::default();
        dispatch_info.definitions.insert(
            "Interface".to_string(),
            Definition {
                kind: DefKind::Trait,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "other_crate".to_string(),
            HashMap::from([
                (String::new(), root_info),
                ("backend::custom".to_string(), custom_info),
                ("dispatch".to_string(), dispatch_info),
            ]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use other_crate::custom::Interface;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "other_crate");
        assert_eq!(dep.target_module, "dispatch");
        assert_eq!(dep.target_item, Some("Interface".to_string()));
    }

    #[test]
    fn test_workspace_import_through_extern_crate_alias() {
        // The importer's root: `extern crate other_crate as oc;`, so `oc` names
        // other_crate in every module of the crate.
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [(
            "other_crate".to_string(),
            HashSet::from(["dispatch".into()]),
        )]
        .into_iter()
        .collect();
        let aliases = HashMap::from([("oc".to_string(), "other_crate".to_string())]);
        let uses = parse_test_uses("use oc::dispatch::Interface;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .current_module_path("user")
            .extern_crates(&aliases)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "other_crate");
        assert_eq!(dep.target_module, "dispatch");
        assert_eq!(dep.target_item, Some("Interface".to_string()));
    }

    #[test]
    fn test_crate_local_import_through_module_from_root_glob() {
        // The crate's own root: `pub mod frame; pub use frame::*;`, and
        // frame/mod.rs holds `pub mod values;`. A module of the same crate
        // imports `crate::values::Value` through the glob.
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["frame".into(), "frame::values".into(), "user".into()]),
        )]
        .into_iter()
        .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.glob_sources.push("frame".to_string());
        let mut values_info = ModuleExportInfo::default();
        values_info.definitions.insert(
            "Value".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([
                (String::new(), root_info),
                ("frame".to_string(), ModuleExportInfo::default()),
                ("frame::values".to_string(), values_info),
            ]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::values::Value;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .module_paths(&mp)
            .current_module_path("user")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "frame::values");
        assert_eq!(dep.target_item, Some("Value".to_string()));
    }

    #[test]
    fn test_crate_local_import_through_root_glob_reexport() {
        // The crate's own root: `mod ty; pub use ty::*;`. A module of the same
        // crate imports `crate::Type`, which only the glob brings into the root.
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["ty".into(), "user".into()]),
        )]
        .into_iter()
        .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.glob_sources.push("ty".to_string());
        let mut ty_info = ModuleExportInfo::default();
        ty_info.definitions.insert(
            "Type".to_string(),
            Definition {
                kind: DefKind::Enum,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([(String::new(), root_info), ("ty".to_string(), ty_info)]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::Type;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .module_paths(&mp)
            .current_module_path("user")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "ty");
        assert_eq!(dep.target_item, Some("Type".to_string()));
    }

    #[test]
    fn test_crate_local_import_through_named_root_reexport() {
        // The crate's own root: `mod eval; pub use eval::{eval_call};`. A module
        // of the same crate imports `crate::eval_call`, a fn, not a module.
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["eval".into(), "user".into()]),
        )]
        .into_iter()
        .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.explicit_reexports.insert(
            "eval_call".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "eval".to_string(),
                original_name: "eval_call".to_string(),
            },
        );
        let mut eval_info = ModuleExportInfo::default();
        eval_info.definitions.insert(
            "eval_call".to_string(),
            Definition {
                kind: DefKind::Fn,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([(String::new(), root_info), ("eval".to_string(), eval_info)]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::eval_call;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .module_paths(&mp)
            .current_module_path("user")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "eval");
        assert_eq!(dep.target_item, Some("eval_call".to_string()));
    }

    #[test]
    fn test_crate_local_import_through_a_reexported_crate() {
        // my_crate's root: `pub use mid_crate::leaf;`; mid_crate's root:
        // `pub use leaf_crate as leaf;`. The path runs through both into
        // leaf_crate.
        let ws: WorkspacePackages = ["my_crate", "mid_crate", "leaf_crate"]
            .into_iter()
            .map(String::from)
            .collect();
        let mp: ModulePathMap = [
            ("my_crate".to_string(), HashSet::from(["user".into()])),
            ("leaf_crate".to_string(), HashSet::from(["error".into()])),
        ]
        .into_iter()
        .collect();
        let mut my_root = ModuleExportInfo::default();
        my_root.explicit_reexports.insert(
            "leaf".to_string(),
            ReExportTarget {
                crate_name: "mid_crate".to_string(),
                module: String::new(),
                original_name: "leaf".to_string(),
            },
        );
        let mut mid_root = ModuleExportInfo::default();
        mid_root.explicit_reexports.insert(
            "leaf".to_string(),
            ReExportTarget {
                crate_name: "leaf_crate".to_string(),
                module: String::new(),
                original_name: String::new(),
            },
        );
        let map: ReExportMap = [
            (
                "my_crate".to_string(),
                HashMap::from([(String::new(), my_root)]),
            ),
            (
                "mid_crate".to_string(),
                HashMap::from([(String::new(), mid_root)]),
            ),
        ]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::leaf::error::ParseError;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .current_module_path("user")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "leaf_crate");
        assert_eq!(dep.target_module, "error");
        assert_eq!(dep.target_item, Some("ParseError".to_string()));
    }

    #[test]
    fn test_workspace_import_through_a_crate_another_root_reexports() {
        // mid_crate's root: `pub use leaf_crate;`. A module of my_crate
        // imports `mid_crate::leaf_crate::error::ParseError`, a path that
        // runs on into leaf_crate.
        let ws: WorkspacePackages = ["my_crate", "mid_crate", "leaf_crate"]
            .into_iter()
            .map(String::from)
            .collect();
        let mp: ModulePathMap = [
            ("my_crate".to_string(), HashSet::from(["user".into()])),
            ("mid_crate".to_string(), HashSet::from(["device".into()])),
            ("leaf_crate".to_string(), HashSet::from(["error".into()])),
        ]
        .into_iter()
        .collect();
        let mut mid_root = ModuleExportInfo::default();
        mid_root.explicit_reexports.insert(
            "leaf_crate".to_string(),
            ReExportTarget {
                crate_name: "leaf_crate".to_string(),
                module: String::new(),
                original_name: String::new(),
            },
        );
        let map: ReExportMap = [(
            "mid_crate".to_string(),
            HashMap::from([(String::new(), mid_root)]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use mid_crate::leaf_crate::error::ParseError;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .current_module_path("user")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        let targets: Vec<_> = deps
            .iter()
            .map(|dep| {
                (
                    dep.target_package.as_str(),
                    dep.target_module.as_str(),
                    dep.target_item.as_deref(),
                )
            })
            .collect();
        assert_eq!(targets, [("leaf_crate", "error", Some("ParseError"))]);
    }

    #[test]
    fn test_crate_local_import_of_item_defined_in_root() {
        // The crate's own root: `mod user; pub struct Server;`. A module of the
        // same crate imports `crate::Server`, defined in the root itself.
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["user".into()]))]
            .into_iter()
            .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.definitions.insert(
            "Server".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 2,
            },
        );
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([(String::new(), root_info)]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::Server;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .module_paths(&mp)
            .current_module_path("user")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "");
        assert_eq!(dep.target_item, Some("Server".to_string()));
    }

    #[test]
    fn test_crate_local_import_of_private_item_in_root() {
        // The crate's own root: `mod user; fn helper() {}`. A private item in
        // the root is visible to every module of the crate.
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["user".into()]))]
            .into_iter()
            .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.private_definitions.insert("helper".to_string());
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([(String::new(), root_info)]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses("use crate::helper;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .module_paths(&mp)
            .current_module_path("user")
            .reexport_map(&map)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "{deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "");
        assert_eq!(dep.target_item, Some("helper".to_string()));
    }

    #[test]
    fn test_cross_crate_no_paths_fallback() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::foo::bar::Baz;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
    use rstest::rstest;

    #[test]
    fn test_parse_workspace_dependencies_mixed() {
        let source = r"
use crate::graph;
use other_crate::utils;
use serde::Serialize;
use std::collections::HashMap;
";
        let ws: WorkspacePackages = ["my_crate".to_string(), "other_crate".to_string()]
            .into_iter()
            .collect();
        let uses = parse_test_uses(source);
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());

        assert_eq!(deps.len(), 2, "found: {deps:?}");
        assert!(
            deps.iter()
                .any(|d| d.target_package == "my_crate" && d.target_module == "graph")
        );
        assert!(
            deps.iter()
                .any(|d| d.target_package == "other_crate" && d.target_module == "utils")
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());

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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "glob should return 1 dep: {deps:?}");
        assert_eq!(deps[0].target_item, Some("*".to_string()));
        assert_eq!(deps[0].target_module, "analyze");
    }

    fn analyze_map() -> ReExportMap {
        let mut analyze_info = ModuleExportInfo::default();
        analyze_info.definitions.insert(
            "Walker".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
        analyze_info.definitions.insert(
            "analyze_module".to_string(),
            Definition {
                kind: DefKind::Fn,
                line: 1,
            },
        );
        [(
            "my_crate".to_string(),
            HashMap::from([("analyze".to_string(), analyze_info)]),
        )]
        .into_iter()
        .collect()
    }

    fn glob_deps(source: &str, map: &ReExportMap) -> Vec<DependencyRef> {
        let ctx = ResolutionContextBuilder::new(Path::new("src/cli.rs"))
            .reexport_map(map)
            .build();
        parse_file_dependencies(
            &syn::parse_file(source).unwrap(),
            &ctx,
            EdgeContext::production(),
        )
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
        let map = analyze_map();
        let deps = glob_deps(
            "use crate::analyze::*;\nfn f(w: Walker) { analyze_module(); }",
            &map,
        );

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
        let map = analyze_map();
        let deps = glob_deps("pub use crate::analyze::*;\nfn f(w: Walker) {}", &map);
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
        let map = analyze_map();
        let deps = glob_deps(
            "use crate::analyze::*;\nfn f() { assert!(analyze_module()); }",
            &map,
        );
        assert_eq!(items_of(&deps), ["analyze_module"], "deps: {deps:?}");
    }

    /// A glob whose names the file never writes keeps the `*`: the import is
    /// there, and the edge with it.
    #[test]
    fn test_process_use_glob_unused_keeps_marker() {
        let map = analyze_map();
        let deps = glob_deps("use crate::analyze::*;\nfn f() {}", &map);
        assert_eq!(items_of(&deps), ["*"], "deps: {deps:?}");
    }

    /// An explicit `use` and an item of the file itself both take the name
    /// away from the glob.
    #[test]
    fn test_process_use_glob_yields_to_explicit_binding_and_local_definition() {
        let map = analyze_map();
        let deps = glob_deps(
            "use crate::analyze::*;\nuse crate::other::Walker;\nfn analyze_module() {}\n\
             fn f(w: Walker) { analyze_module(); }",
            &map,
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
        let map = analyze_map();
        let deps = glob_deps(&format!("use crate::analyze::*;\n{body}"), &map);
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
        let map = analyze_map();
        let deps = glob_deps(&format!("use crate::analyze::*;\n{body}"), &map);
        assert_eq!(items_of(&deps), ["analyze_module"], "deps: {deps:?}");
    }

    /// A glob written in a block binds in that block only.
    #[test]
    fn test_process_use_glob_binds_in_its_region() {
        let map = analyze_map();
        let deps = glob_deps(
            "fn f() { use crate::analyze::*; analyze_module(); }\nfn g() { analyze_module(); }",
            &map,
        );
        assert_eq!(items_of(&deps), ["analyze_module"], "deps: {deps:?}");
        assert_eq!(deps[0].uses.len(), 1, "the call in `g` is not the glob's");
    }

    /// A glob re-export republishes the names it pulls in, so each expanded name
    /// resolves on to the module that defines it — same as a named `pub use`.
    #[test]
    fn test_process_use_glob_expands_through_reexport() {
        let mut facade_info = ModuleExportInfo::default();
        facade_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "origin".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut origin_info = ModuleExportInfo::default();
        origin_info.definitions.insert(
            "Widget".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([
                ("facade".to_string(), facade_info),
                ("origin".to_string(), origin_info),
            ]),
        )]
        .into_iter()
        .collect();

        let deps = glob_deps("use crate::facade::*;\nfn f(w: Widget) {}", &map);

        assert_eq!(deps.len(), 1, "deps: {deps:?}");
        assert_eq!(deps[0].target_item, Some("Widget".to_string()));
        assert_eq!(deps[0].target_module, "origin");
    }

    /// An unknown payload keeps the `*`: one unnamed symbol, not zero.
    #[test]
    fn test_process_use_glob_unknown_payload_keeps_marker() {
        let deps = glob_deps(
            "use crate::analyze::*;\nfn f(w: Walker) {}",
            &ReExportMap::default(),
        );

        assert_eq!(deps.len(), 1, "deps: {deps:?}");
        assert_eq!(deps[0].target_item, Some("*".to_string()));
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
        let ws: WorkspacePackages = ["my_crate".to_string(), "other_crate".to_string()]
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());

        // Expected: 6 DependencyRefs
        // - cli::Args, cli::Cargo, cli::run (bare module, pub use multi)
        // - graph::build (crate:: prefix)
        // - model::Node (bare module, pub(crate))
        // - other_crate::utils (workspace crate)
        assert_eq!(deps.len(), 6, "expected 6 deps, got: {deps:?}");

        // Bare module multi-import (pub use)
        assert!(
            deps.iter().any(|d| d.target_package == "my_crate"
                && d.target_module == "cli"
                && d.target_item == Some("Args".to_string())),
            "missing cli::Args"
        );
        assert!(
            deps.iter().any(|d| d.target_package == "my_crate"
                && d.target_module == "cli"
                && d.target_item == Some("Cargo".to_string())),
            "missing cli::Cargo"
        );
        assert!(
            deps.iter().any(|d| d.target_package == "my_crate"
                && d.target_module == "cli"
                && d.target_item == Some("run".to_string())),
            "missing cli::run"
        );

        // crate:: prefix
        assert!(
            deps.iter().any(|d| d.target_package == "my_crate"
                && d.target_module == "graph"
                && d.target_item == Some("build".to_string())),
            "missing graph::build"
        );

        // Bare module pub(crate)
        assert!(
            deps.iter().any(|d| d.target_package == "my_crate"
                && d.target_module == "model"
                && d.target_item == Some("Node".to_string())),
            "missing model::Node"
        );

        // Workspace crate
        assert!(
            deps.iter()
                .any(|d| d.target_package == "other_crate" && d.target_module == "utils"),
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
        assert_eq!(dep.target_package, "my_crate");
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
        assert_eq!(dep.target_package, "my_crate");
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
        assert_eq!(dep.target_package, "my_crate");
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "cli");
        assert_eq!(dep.target_item, Some("Args".to_string()));
        assert!(!dep.via_reexport);
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "cli");
        assert_eq!(dep.target_item, Some("Args".to_string()));
        assert!(dep.via_reexport);
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 3, "should return 3 deps: {deps:?}");
        assert!(deps.iter().all(|d| d.target_package == "my_crate"));
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "glob should return 1 dep: {deps:?}");
        assert_eq!(deps[0].target_package, "my_crate");
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
        assert_eq!(dep.target_package, "my_crate");
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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

    /// A top-level module is in scope of the crate root only. From a sibling
    /// file its bare name is not the module; without a `use` it can only be an
    /// extern crate, so nothing of this crate resolves.
    #[test]
    fn test_bare_module_root_sibling_not_in_scope() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["config".into(), "error".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/error.rs"))
            .module_paths(&mp)
            .current_module_path("error")
            .build();
        let dep =
            parse_bare_module_import(&ctx, "config::ConfigError", 1, &EdgeContext::production());
        assert!(dep.is_none(), "a root sibling is not in scope: {dep:?}");
    }

    /// The same collision seen through a file: `config::ConfigError` in
    /// `error.rs` beside a module `config` yields no edge to that module.
    #[test]
    fn test_root_sibling_path_ref_is_no_dependency() {
        let source = r"
impl From<config::ConfigError> for ConfigError {
    fn from(e: config::ConfigError) -> Self { ConfigError::Load(e) }
}
";
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["config".into(), "error".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/error.rs"))
            .module_paths(&mp)
            .current_module_path("error")
            .build();
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        let bindings = bindings_of(source, &ctx);
        let deps = parse_path_ref_dependencies(&refs, &ctx, &bindings);
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

    #[test]
    fn test_entry_point_export_detected() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
            .into_iter()
            .collect();
        let exports: PackageExportMap = [(
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_package, "other_crate");
        assert_eq!(dep.target_module, "");
        assert_eq!(dep.target_item, Some("MyStruct".to_string()));
    }

    #[test]
    fn test_non_export_stays_fallback() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
            .into_iter()
            .collect();
        let exports: PackageExportMap = [("other_crate".to_string(), HashSet::new())]
            .into_iter()
            .collect();
        let uses = parse_test_uses("use other_crate::Unknown;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .crate_exports(&exports)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.target_module, "Unknown");
        assert!(dep.target_item.is_none());
    }

    #[test]
    fn test_real_module_not_affected() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
            .into_iter()
            .collect();
        let exports: PackageExportMap =
            [("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]
                .into_iter()
                .collect();
        let uses = parse_test_uses("use other_crate::sub_mod::Foo;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .crate_exports(&exports)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::{Foo, Bar};");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 2, "should return 2 deps: {deps:?}");
        assert!(deps.iter().all(|d| d.target_package == "other_crate"));
        assert!(deps.iter().any(|d| d.target_module == "Foo"));
        assert!(deps.iter().any(|d| d.target_module == "Bar"));
    }

    #[test]
    fn test_entry_point_glob_import() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let uses = parse_test_uses("use other_crate::*;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
fn main() {
    let x = helper();
}
";
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["helper".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/main.rs"))
            .module_paths(&mp)
            .build();
        let syntax = syn::parse_file(source).unwrap();
        let refs = collect_all_path_refs(&syntax, EdgeContext::production());
        assert!(
            refs.iter().any(|r| r.path == "helper"),
            "the bare name is collected: {refs:?}"
        );
        let bindings = bindings_of("", &ctx);
        let deps = parse_path_ref_dependencies(&refs, &ctx, &bindings);
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

mod path_ref_resolution_tests {
    use super::*;
    use crate::model::EdgeContext;

    #[test]
    fn test_parse_path_refs_workspace_crate() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
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
        let deps = resolve_root_refs(paths, &ctx);
        assert_eq!(
            deps.len(),
            1,
            "should resolve workspace crate path: {deps:?}"
        );
        assert_eq!(deps[0].target_package, "other_crate");
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
        let deps = resolve_root_refs(paths, &ctx);
        assert_eq!(deps.len(), 1, "should resolve crate-local path: {deps:?}");
        assert_eq!(deps[0].target_package, "my_crate");
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
        let deps = resolve_root_refs(paths, &ctx);
        assert_eq!(deps.len(), 1, "should resolve bare module path: {deps:?}");
        assert_eq!(deps[0].target_package, "my_crate");
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
        let deps = resolve_root_refs(paths, &ctx);
        assert!(deps.is_empty(), "unknown paths should be skipped: {deps:?}");
    }

    #[test]
    fn test_parse_path_refs_entry_point() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let exports: PackageExportMap = [(
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
        let deps = resolve_root_refs(paths, &ctx);
        assert_eq!(deps.len(), 1, "should resolve entry-point path: {deps:?}");
        assert_eq!(deps[0].target_package, "other_crate");
        assert_eq!(deps[0].target_module, "");
        assert_eq!(deps[0].target_item, Some("MyStruct".to_string()));
    }

    #[test]
    fn test_parse_path_refs_dedup() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
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
        let deps = resolve_root_refs(paths, &ctx);
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
        // Realistic: `use super::filtering::DependencyInfo` from analyze::workspace
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
        let uses = parse_test_uses("use super::filtering::DependencyInfo;");
        let ctx = ResolutionContextBuilder::new(Path::new("src/analyze/workspace.rs"))
            .module_paths(&mp)
            .current_module_path("analyze::workspace")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "super::filtering should resolve: {deps:?}");
        let dep = &deps[0];
        assert_eq!(dep.target_package, "my_crate");
        assert_eq!(dep.target_module, "analyze::filtering");
        assert_eq!(dep.target_item, Some("DependencyInfo".to_string()));
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(
            deps.len(),
            2,
            "super::use_parser multi-import should resolve: {deps:?}"
        );
        assert!(deps.iter().all(|d| d.target_package == "my_crate"));
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "self::child should resolve: {deps:?}");
        assert_eq!(deps[0].target_package, "my_crate");
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(
            deps.len(),
            1,
            "super::super from inline mod should create real edge: {deps:?}"
        );
        assert_eq!(deps[0].target_module, "analyze::sibling");
        assert_eq!(deps[0].target_item, Some("Item".to_string()));
    }

    #[test]
    fn super_from_inline_mod_reaches_a_child_module_of_the_file() {
        // `pub mod api { pub use super::backend::Api; }` in render/mod.rs:
        // `super` leaves the inline module, `backend` is a child of `render`.
        let source = r"
pub mod api {
    pub use super::backend::Api;
    pub use super::*;
    pub use super::helper;
}
";
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["render".into(), "render::backend".into()]),
        )]
        .into_iter()
        .collect();
        let uses = parse_test_uses(source);
        let ctx = ResolutionContextBuilder::new(Path::new("src/render/mod.rs"))
            .module_paths(&mp)
            .current_module_path("render")
            .build();
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "only the child module is a target: {deps:?}");
        assert_eq!(deps[0].target_module, "render::backend");
        assert_eq!(deps[0].target_item, Some("Api".to_string()));
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
        assert_eq!(uses[0].inline_depth, 0, "top-level use should have depth 0");
        // use inside mod inner: depth 1
        assert_eq!(
            uses[1].inline_depth, 1,
            "use in mod inner should have depth 1"
        );
        // use inside mod inner::deep: depth 2
        assert_eq!(
            uses[2].inline_depth, 2,
            "use in mod deep should have depth 2"
        );
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
        let deps = parse_workspace_dependencies(&root_uses(uses), &ctx, &FileBindings::default());
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
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
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
        let deps = resolve_root_refs(paths, &ctx);
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
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
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
        let deps = resolve_root_refs(paths, &ctx);
        assert_eq!(
            deps.len(),
            1,
            "same context same target should still dedup: {deps:?}"
        );
    }
}

mod reexport_resolution_tests {
    use super::super::{ModuleExportInfo, ReExportMap, ReExportTarget};
    use super::*;

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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
                crate_name: "my_crate".to_string(),
                module: "parent::sibling".to_string(),
                original_name: "Item".to_string(),
            },
        );
        let mut sibling_info = ModuleExportInfo::default();
        sibling_info.definitions.insert(
            "Item".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
                crate_name: "my_crate".to_string(),
                module: "parent::child".to_string(),
                original_name: "Config".to_string(),
            },
        );
        let mut child_info = ModuleExportInfo::default();
        child_info.definitions.insert(
            "Config".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
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
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
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
                crate_name: "my_crate".to_string(),
                module: "parent::child".to_string(),
                original_name: "Config".to_string(),
            },
        );
        let mut child_info = ModuleExportInfo::default();
        child_info.definitions.insert(
            "Config".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
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
        let deps = resolve_root_refs(paths, &ctx);
        assert_eq!(deps.len(), 1);
        assert_eq!(
            deps[0].target_module, "parent::child",
            "path refs should also resolve through re-export"
        );
    }

    /// `pub use helper::helper;` next to a private `mod helper;`: a path
    /// ending in that name from another crate names the fn the module
    /// re-exports, not the private module.
    #[test]
    fn path_naming_a_module_and_the_item_its_parent_reexports_takes_the_item() {
        let mp: ModulePathMap = [(
            "core_crate".to_string(),
            HashSet::from(["util".into(), "util::helper".into()]),
        )]
        .into_iter()
        .collect();
        let mut util_info = ModuleExportInfo::default();
        util_info.explicit_reexports.insert(
            "helper".to_string(),
            ReExportTarget {
                crate_name: "core_crate".to_string(),
                module: "util::helper".to_string(),
                original_name: "helper".to_string(),
            },
        );
        let mut helper_info = ModuleExportInfo::default();
        helper_info.definitions.insert(
            "helper".to_string(),
            Definition {
                kind: DefKind::Fn,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "core_crate".to_string(),
            HashMap::from([
                ("util".to_string(), util_info),
                ("util::helper".to_string(), helper_info),
            ]),
        )]
        .into_iter()
        .collect();
        let ws: WorkspacePackages = ["core_crate".to_string()].into_iter().collect();

        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .reexport_map(&map)
            .build();
        let deps = parse_file_dependencies(
            &syn::parse_file("use core_crate::util::helper;\nfn f() { helper(); }").unwrap(),
            &ctx,
            EdgeContext::production(),
        );

        let targets: Vec<_> = deps
            .iter()
            .map(|d| {
                (
                    d.target_module.as_str(),
                    d.target_item.as_deref(),
                    d.uses.len(),
                )
            })
            .collect();
        assert_eq!(targets, [("util::helper", Some("helper"), 1)], "{deps:?}");
    }

    /// `my_crate` with `pub struct Set;` in `store::set`, re-exported by
    /// `store`, and a module `holder` that binds the name `bound` to the
    /// module `store`, privately or with `pub use`.
    fn map_with_a_module_bound_in_holder(private: bool) -> (ModulePathMap, ReExportMap) {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "store".into(),
                "store::set".into(),
                "holder".into(),
                "holder::user".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let bound = ReExportTarget {
            crate_name: "my_crate".to_string(),
            module: String::new(),
            original_name: "store".to_string(),
        };
        let mut holder_info = ModuleExportInfo::default();
        if private {
            holder_info.private_uses.insert("bound".to_string(), bound);
        } else {
            holder_info
                .explicit_reexports
                .insert("bound".to_string(), bound);
        }
        let mut store_info = ModuleExportInfo::default();
        store_info.explicit_reexports.insert(
            "Set".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "store::set".to_string(),
                original_name: "Set".to_string(),
            },
        );
        let mut set_info = ModuleExportInfo::default();
        set_info.definitions.insert(
            "Set".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([
                ("holder".to_string(), holder_info),
                ("store".to_string(), store_info),
                ("store::set".to_string(), set_info),
            ]),
        )]
        .into_iter()
        .collect();
        (mp, map)
    }

    fn targets_of(
        source: &str,
        current_module: &str,
        private: bool,
    ) -> Vec<(String, Option<String>, usize)> {
        let (mp, map) = map_with_a_module_bound_in_holder(private);
        let ctx = ResolutionContextBuilder::new(Path::new("src/holder/user.rs"))
            .module_paths(&mp)
            .current_module_path(current_module)
            .reexport_map(&map)
            .build();
        parse_file_dependencies(
            &syn::parse_file(source).unwrap(),
            &ctx,
            EdgeContext::production(),
        )
        .into_iter()
        .map(|d| (d.target_module, d.target_item, d.uses.len()))
        .collect()
    }

    #[test]
    fn path_through_a_private_module_binding_of_an_ancestor_keeps_its_item() {
        let targets = targets_of(
            "use super::bound::Set;\nfn f(_: Set) {}",
            "holder::user",
            true,
        );
        assert_eq!(
            targets,
            [("store::set".to_string(), Some("Set".to_string()), 1)]
        );
    }

    #[test]
    fn path_through_a_reexported_module_keeps_its_item() {
        let targets = targets_of("use crate::holder::bound::Set;\nfn f(_: Set) {}", "", false);
        assert_eq!(
            targets,
            [("store::set".to_string(), Some("Set".to_string()), 1)]
        );
    }
}

mod resolve_reexport_tests {
    use super::super::{
        ModuleExportInfo, ModulePathMap, ReExportMap, ReExportTarget, resolve_reexport,
    };
    use super::DEFAULT_MP;
    use crate::model::{DefKind, Definition, DependencyRef, EdgeContext};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn test_dep(crate_name: &str, module: &str, item: Option<&str>) -> DependencyRef {
        DependencyRef {
            target_package: crate_name.to_string(),
            target_module: module.to_string(),
            target_item: item.map(String::from),
            source_file: PathBuf::from("src/lib.rs"),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
            uses: Vec::new(),
            in_lib_view: false,
        }
    }

    // --- resolve_reexport: Cycle 1 — No target_item → dep unchanged ---

    #[test]
    fn resolve_noop_without_target_item() {
        let map = ReExportMap::default();
        let mut dep = test_dep("my_crate", "render", None);
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "render");
    }

    #[test]
    fn resolve_reexport_of_a_module_ends_on_the_module() {
        let mut root = ModuleExportInfo::default();
        root.explicit_reexports.insert(
            "custom".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "backend".to_string(),
                original_name: "custom".to_string(),
            },
        );
        root.explicit_reexports.insert(
            "leaf".to_string(),
            ReExportTarget {
                crate_name: "leaf_crate".to_string(),
                module: String::new(),
                original_name: String::new(),
            },
        );
        let mut custom = ModuleExportInfo::default();
        custom.glob_sources.push("dispatch".to_string());
        let mut leaf_root = ModuleExportInfo::default();
        leaf_root.glob_sources.push("ir".to_string());
        let map: ReExportMap = [
            (
                "my_crate".to_string(),
                HashMap::from([
                    (String::new(), root),
                    ("backend::custom".to_string(), custom),
                ]),
            ),
            (
                "leaf_crate".to_string(),
                HashMap::from([(String::new(), leaf_root)]),
            ),
        ]
        .into_iter()
        .collect();
        let module_paths: ModulePathMap = [
            (
                "my_crate".to_string(),
                HashSet::from(["backend".to_string(), "backend::custom".to_string()]),
            ),
            ("leaf_crate".to_string(), HashSet::from(["ir".to_string()])),
        ]
        .into_iter()
        .collect();

        let mut module_dep = test_dep("my_crate", "", Some("custom"));
        resolve_reexport(&mut module_dep, &map, &module_paths, "api");
        let mut crate_dep = test_dep("my_crate", "", Some("leaf"));
        resolve_reexport(&mut crate_dep, &map, &module_paths, "api");

        assert_eq!(
            (module_dep.target_module.as_str(), module_dep.target_item),
            ("backend::custom", None)
        );
        assert_eq!(
            (
                crate_dep.target_package.as_str(),
                crate_dep.target_module.as_str(),
                crate_dep.target_item
            ),
            ("leaf_crate", "", None)
        );
    }

    #[test]
    fn resolve_private_use_of_a_module_ends_on_the_module() {
        let mut parent = ModuleExportInfo::default();
        parent.private_uses.insert(
            "arena".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: String::new(),
                original_name: "arena".to_string(),
            },
        );
        let mut arena = ModuleExportInfo::default();
        arena.definitions.insert(
            "HandleSet".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([
                ("compact".to_string(), parent),
                ("arena".to_string(), arena),
            ]),
        )]
        .into_iter()
        .collect();
        let module_paths: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "compact".to_string(),
                "compact::functions".to_string(),
                "arena".to_string(),
            ]),
        )]
        .into_iter()
        .collect();

        let mut dep = test_dep("my_crate", "compact", Some("arena"));
        resolve_reexport(&mut dep, &map, &module_paths, "compact::functions");

        assert_eq!(
            (dep.target_module.as_str(), dep.target_item),
            ("arena", None)
        );
    }

    // --- resolve_reexport: Cycle 2 — Crate not in map → dep unchanged ---

    #[test]
    fn resolve_noop_when_crate_not_in_map() {
        let map = ReExportMap::default();
        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "render");
    }

    // --- resolve_reexport: Cycle 3 — Own definition → dep unchanged ---

    #[test]
    fn resolve_noop_when_own_definition() {
        let mut module_info = ModuleExportInfo::default();
        module_info.definitions.insert(
            "Widget".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
        let mut crate_exports = HashMap::new();
        crate_exports.insert("render".to_string(), module_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "render");
    }

    // --- resolve_reexport: Cycle 4 — Explicit re-export → target_module updated ---

    #[test]
    fn resolve_explicit_reexport() {
        let mut m_info = ModuleExportInfo::default();
        m_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "render::elements".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut n_info = ModuleExportInfo::default();
        n_info.definitions.insert(
            "Widget".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("render".to_string(), m_info);
        crate_exports.insert("render::elements".to_string(), n_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "render::elements");
    }

    // --- resolve_reexport: Cycle 5 — Transitive chain M → N → O ---

    #[test]
    fn resolve_transitive_chain() {
        let mut m_info = ModuleExportInfo::default();
        m_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "middle".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut n_info = ModuleExportInfo::default();
        n_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "origin".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut o_info = ModuleExportInfo::default();
        o_info.definitions.insert(
            "Widget".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("root".to_string(), m_info);
        crate_exports.insert("middle".to_string(), n_info);
        crate_exports.insert("origin".to_string(), o_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "root", Some("Widget"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "origin");
    }

    // --- resolve_reexport: Cycle 6 — Rename chain follows original_name ---

    #[test]
    fn resolve_rename_chain() {
        let mut m_info = ModuleExportInfo::default();
        m_info.explicit_reexports.insert(
            "Alias".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "origin".to_string(),
                original_name: "Original".to_string(),
            },
        );
        let mut o_info = ModuleExportInfo::default();
        o_info.definitions.insert(
            "Original".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("facade".to_string(), m_info);
        crate_exports.insert("origin".to_string(), o_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "facade", Some("Alias"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "origin");
    }

    // --- resolve_reexport: Cycle 7 — Glob resolution ---

    #[test]
    fn resolve_glob_reexport() {
        let mut m_info = ModuleExportInfo::default();
        m_info.glob_sources.push("elements".to_string());
        let mut n_info = ModuleExportInfo::default();
        n_info.definitions.insert(
            "Widget".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("render".to_string(), m_info);
        crate_exports.insert("elements".to_string(), n_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "elements");
    }

    // --- resolve_reexport: Cycle 8 — Glob + transitive ---

    #[test]
    fn resolve_glob_then_explicit() {
        let mut m_info = ModuleExportInfo::default();
        m_info.glob_sources.push("middle".to_string());
        let mut n_info = ModuleExportInfo::default();
        n_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "origin".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut o_info = ModuleExportInfo::default();
        o_info.definitions.insert(
            "Widget".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("facade".to_string(), m_info);
        crate_exports.insert("middle".to_string(), n_info);
        crate_exports.insert("origin".to_string(), o_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "facade", Some("Widget"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "origin");
    }

    // --- resolve_reexport: Cycle 9 — Cycle guard (no infinite loop) ---

    #[test]
    fn resolve_cycle_guard() {
        let mut a_info = ModuleExportInfo::default();
        a_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "b".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut b_info = ModuleExportInfo::default();
        b_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "a".to_string(),
                original_name: "Widget".to_string(),
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("a".to_string(), a_info);
        crate_exports.insert("b".to_string(), b_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "a", Some("Widget"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        // Terminates without infinite loop; ends back at "a"
        assert_eq!(dep.target_module, "a");
    }

    // --- resolve_reexport: Cycle 10 — Cross-crate resolution ---

    #[test]
    fn resolve_cross_crate() {
        let mut root_info = ModuleExportInfo::default();
        root_info.explicit_reexports.insert(
            "Config".to_string(),
            ReExportTarget {
                crate_name: "other_crate".to_string(),
                module: "settings".to_string(),
                original_name: "Config".to_string(),
            },
        );
        let mut settings_info = ModuleExportInfo::default();
        settings_info.definitions.insert(
            "Config".to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert(String::new(), root_info);
        crate_exports.insert("settings".to_string(), settings_info);
        let map: ReExportMap = [("other_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("other_crate", "", Some("Config"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "");
        assert_eq!(dep.target_module, "settings");
    }

    // --- resolve_reexport: private use, descendant visibility ---

    #[test]
    fn resolve_private_use_for_descendant() {
        // front::wgsl holds a private `use ...::error::Error`; a descendant
        // module names it through the ancestor. The edge belongs to the definer.
        let mut wgsl_info = ModuleExportInfo::default();
        wgsl_info.private_uses.insert(
            "Error".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "front::wgsl::error".to_string(),
                original_name: "Error".to_string(),
            },
        );
        let mut error_info = ModuleExportInfo::default();
        error_info.definitions.insert(
            "Error".to_string(),
            Definition {
                kind: DefKind::Enum,
                line: 1,
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("front::wgsl".to_string(), wgsl_info);
        crate_exports.insert("front::wgsl::error".to_string(), error_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "front::wgsl", Some("Error"));
        resolve_reexport(
            &mut dep,
            &map,
            &DEFAULT_MP,
            "front::wgsl::parse::directive::enable_extension",
        );
        assert_eq!(dep.target_module, "front::wgsl::error");
    }

    #[test]
    fn resolve_private_use_not_for_non_descendant() {
        // A module outside front::wgsl cannot see its private `use`; without a
        // visible binding the edge stays on the ancestor.
        let mut wgsl_info = ModuleExportInfo::default();
        wgsl_info.private_uses.insert(
            "Error".to_string(),
            ReExportTarget {
                crate_name: "my_crate".to_string(),
                module: "front::wgsl::error".to_string(),
                original_name: "Error".to_string(),
            },
        );
        let mut crate_exports = HashMap::new();
        crate_exports.insert("front::wgsl".to_string(), wgsl_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "front::wgsl", Some("Error"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "back::spv");
        assert_eq!(dep.target_module, "front::wgsl");
    }

    // --- resolve_reexport: private glob, descendant visibility ---

    fn private_glob_map(holder: &str, source: &str, item: &str) -> ReExportMap {
        [(
            "my_crate".to_string(),
            private_glob_modules(holder, source, item),
        )]
        .into_iter()
        .collect()
    }

    fn private_glob_modules(
        holder: &str,
        source: &str,
        item: &str,
    ) -> HashMap<String, ModuleExportInfo> {
        let mut holder_info = ModuleExportInfo::default();
        holder_info.private_glob_sources.push(source.to_string());
        let mut source_info = ModuleExportInfo::default();
        source_info.definitions.insert(
            item.to_string(),
            Definition {
                kind: DefKind::Struct,
                line: 1,
            },
        );
        let mut crate_exports = HashMap::new();
        crate_exports.insert(holder.to_string(), holder_info);
        crate_exports.insert(source.to_string(), source_info);
        crate_exports
    }

    #[test]
    fn resolve_private_glob_for_descendant() {
        // api holds a private `use actions::*` and names nothing from it; a
        // descendant names Mapping through the ancestor. The edge belongs to
        // the definer.
        let map = private_glob_map("api", "api::actions", "Mapping");

        let mut dep = test_dep("my_crate", "api", Some("Mapping"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "api::buffer");
        assert_eq!(dep.target_module, "api::actions");
    }

    #[test]
    fn resolve_private_glob_not_for_non_descendant() {
        // A module outside api cannot see its private glob; without a visible
        // binding the edge stays on the ancestor.
        let map = private_glob_map("api", "api::actions", "Mapping");

        let mut dep = test_dep("my_crate", "api", Some("Mapping"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "backend");
        assert_eq!(dep.target_module, "api");
    }

    #[test]
    fn resolve_private_glob_chain_for_descendant() {
        // api::buffer forwards api's scope with `use super::*`, api forwards
        // actions with `use actions::*`; a module below api::buffer reaches
        // the definer through both.
        let mut modules = private_glob_modules("api", "api::actions", "Mapping");
        let mut buffer_info = ModuleExportInfo::default();
        buffer_info.private_glob_sources.push("api".to_string());
        modules.insert("api::buffer".to_string(), buffer_info);
        let map: ReExportMap = [("my_crate".to_string(), modules)].into_iter().collect();

        let mut dep = test_dep("my_crate", "api::buffer", Some("Mapping"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "api::buffer::view");
        assert_eq!(dep.target_module, "api::actions");
    }

    #[test]
    fn resolve_pub_glob_carries_none_of_the_sources_private_globs() {
        // dispatch holds private globs on custom and core; custom republishes
        // dispatch with `pub use dispatch::*`. That carries dispatch's public
        // items only, so Mapping (defined in core) is not custom's to give,
        // even to dispatch itself.
        let mut modules = private_glob_modules("dispatch", "core", "Mapping");
        let mut custom_info = ModuleExportInfo::default();
        custom_info.glob_sources.push("dispatch".to_string());
        modules.insert("custom".to_string(), custom_info);
        modules
            .get_mut("dispatch")
            .unwrap()
            .private_glob_sources
            .push("custom".to_string());
        let map: ReExportMap = [("my_crate".to_string(), modules)].into_iter().collect();

        let mut dep = test_dep("my_crate", "custom", Some("Mapping"));
        resolve_reexport(&mut dep, &map, &DEFAULT_MP, "dispatch");
        assert_eq!(dep.target_module, "custom");
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
        let ext_names: HashMap<String, String> =
            [("serde".to_string(), "serde-pkg-id".to_string())]
                .into_iter()
                .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .external_crate_names(&ext_names)
            .build();
        let dep = resolve_single_path(&ctx, "serde::Deserialize", 1, &EdgeContext::production(), 0);
        assert!(dep.is_some(), "should resolve external crate import");
        let dep = dep.unwrap();
        assert_eq!(dep.target_package, "serde");
        assert_eq!(dep.target_item, Some("Deserialize".to_string()));
    }

    #[test]
    fn test_resolve_external_does_not_shadow_workspace() {
        let ws: WorkspacePackages = ["serde"].into_iter().collect();
        let ext_names: HashMap<String, String> =
            [("serde".to_string(), "serde-pkg-id".to_string())]
                .into_iter()
                .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .workspace_crates(&ws)
            .external_crate_names(&ext_names)
            .build();
        // With serde as both workspace crate and external, workspace should win
        let dep = resolve_single_path(&ctx, "serde::Foo", 1, &EdgeContext::production(), 0);
        assert!(dep.is_some());
        let dep = dep.unwrap();
        // Workspace import resolution takes priority
        assert_eq!(dep.target_package, "serde");
    }

    #[test]
    fn test_resolve_external_no_match() {
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs")).build();
        let dep = resolve_single_path(&ctx, "unknown_crate::Foo", 1, &EdgeContext::production(), 0);
        assert!(dep.is_none(), "unknown crate should not resolve");
    }
}

/// `use crate::parent::child` binds `child` locally; later `child::Item` paths
/// must resolve to `parent::child`, not to a top-level `child`.
mod file_binding_tests {
    use super::*;
    use rstest::rstest;

    fn nested_module_paths() -> ModulePathMap {
        [(
            "my_crate".to_string(),
            HashSet::from(["parent".into(), "parent::child".into()]),
        )]
        .into_iter()
        .collect()
    }

    #[test]
    fn test_alias_map_binds_imported_module() {
        let mp = nested_module_paths();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use crate::parent::child;", &ctx);
        assert_eq!(
            bindings.target(BindingRegions::ROOT, "child"),
            Some("crate::parent::child"),
            "leaf name should bind to the full module path: {bindings:?}"
        );
    }

    #[test]
    fn test_alias_map_binds_group_member() {
        let mp = nested_module_paths();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use crate::parent::{child, Other};", &ctx);
        assert_eq!(
            bindings.target(BindingRegions::ROOT, "child"),
            Some("crate::parent::child")
        );
        assert!(
            bindings.target(BindingRegions::ROOT, "Other").is_none(),
            "non-module items must not enter the alias map: {bindings:?}"
        );
    }

    #[test]
    fn test_alias_map_uses_rename_as_key() {
        let mp = nested_module_paths();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use crate::parent::child as kid;", &ctx);
        assert_eq!(
            bindings.target(BindingRegions::ROOT, "kid"),
            Some("crate::parent::child"),
            "rename binds the alias name to the original module: {bindings:?}"
        );
    }

    #[test]
    fn test_alias_map_binds_workspace_module() {
        let ws: WorkspacePackages = ["other_crate".to_string()].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use other_crate::module;", &ctx);
        assert_eq!(
            bindings.target(BindingRegions::ROOT, "module"),
            Some("other_crate::module"),
            "workspace module alias keeps the code-side crate name: {bindings:?}"
        );
    }

    #[test]
    fn test_qualified_use_through_alias_resolves() {
        let mp = nested_module_paths();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use crate::parent::child;", &ctx);
        let paths = vec![("child::Item".to_string(), 20, EdgeContext::production(), 0)];
        let deps = parse_path_ref_dependencies(&root_path_refs(paths), &ctx, &bindings);
        assert_eq!(deps.len(), 1, "alias-qualified path must resolve: {deps:?}");
        assert_eq!(deps[0].target_module, "parent::child");
        assert_eq!(deps[0].target_item, Some("Item".to_string()));
    }

    #[test]
    fn test_qualified_use_through_alias_keeps_deeper_segments() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "parent".into(),
                "parent::child".into(),
                "parent::child::inner".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use crate::parent::child;", &ctx);
        let paths = vec![(
            "child::inner::Item".to_string(),
            20,
            EdgeContext::production(),
            0,
        )];
        let deps = parse_path_ref_dependencies(&root_path_refs(paths), &ctx, &bindings);
        assert_eq!(deps.len(), 1, "should resolve through alias: {deps:?}");
        assert_eq!(deps[0].target_module, "parent::child::inner");
        assert_eq!(deps[0].target_item, Some("Item".to_string()));
    }

    #[test]
    fn test_alias_does_not_shadow_real_module() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["parent".into(), "parent::child".into(), "child".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .build();
        let paths = vec![("child::Item".to_string(), 20, EdgeContext::production(), 0)];
        let deps = resolve_root_refs(paths, &ctx);
        assert_eq!(deps.len(), 1, "top-level module still resolves: {deps:?}");
        assert_eq!(deps[0].target_module, "child");
    }

    fn colliding_module_paths() -> ModulePathMap {
        [(
            "my_crate".to_string(),
            HashSet::from(["consumer".into(), "shared".into()]),
        )]
        .into_iter()
        .collect()
    }

    fn foreign_crate_names() -> HashMap<String, String> {
        [("remote_lib".to_string(), "remote_lib 1.0".to_string())]
            .into_iter()
            .collect()
    }

    /// The name can collide through either half of the `use`: its last path
    /// segment or the alias it is renamed to.
    #[rstest]
    #[case::last_segment("use remote_lib::shared;")]
    #[case::alias_name("use remote_lib as shared;")]
    fn test_binding_from_another_crate_beats_same_named_module(#[case] use_line: &str) {
        let mp = colliding_module_paths();
        let ext = foreign_crate_names();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .current_module_path("consumer")
            .external_crate_names(&ext)
            .build();
        let bindings = bindings_of(use_line, &ctx);
        let paths = vec![("shared::Item".to_string(), 20, EdgeContext::production(), 0)];
        let deps = parse_path_ref_dependencies(&root_path_refs(paths), &ctx, &bindings);
        assert_eq!(deps.len(), 1, "the reference resolves once: {deps:?}");
        assert_eq!(
            deps[0].target_package, "remote_lib",
            "the binding names the other crate, not the module beside the file: {deps:?}"
        );
    }

    /// In the crate root, where the module is in scope by its bare name.
    #[test]
    fn test_item_import_from_this_crate_leaves_the_module_alone() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["util".into(), "helper".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/lib.rs"))
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use crate::util::helper;", &ctx);
        let paths = vec![(
            "helper::thing".to_string(),
            20,
            EdgeContext::production(),
            0,
        )];
        let deps = parse_path_ref_dependencies(&root_path_refs(paths), &ctx, &bindings);
        assert_eq!(
            deps.len(),
            1,
            "an item import does not take the name away from the module: {deps:?}"
        );
        assert_eq!(deps[0].target_module, "helper");
    }

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

    /// A `use` binds where it is written. One inside a function body says nothing
    /// about the function beside it, where the bare name still means the module
    /// next to the file.
    #[test]
    fn test_binding_in_a_function_body_leaves_the_neighbour_module_alone() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "render".into(),
                "render::queue".into(),
                "metrics".into(),
                "metrics::queue".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/render/mod.rs"))
            .module_paths(&mp)
            .current_module_path("render")
            .build();
        let file = syn::parse_file(
            "fn draw() { queue::submit(); }\n\
             fn stats() { use crate::metrics::queue; queue::len(); }",
        )
        .unwrap();
        let uses = collect_all_use_items(&file, EdgeContext::production());
        let refs = collect_all_path_refs(&file, EdgeContext::production());

        let bindings = FileBindings::of(&file, &uses, &ctx);
        let deps = parse_path_ref_dependencies(&refs, &ctx, &bindings);

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
        let mp = colliding_module_paths();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .current_module_path("consumer")
            .build();
        let bindings = bindings_of("use remote_lib::shared;", &ctx);
        let paths = vec![("shared::Item".to_string(), 20, EdgeContext::production(), 0)];
        let deps = parse_path_ref_dependencies(&root_path_refs(paths), &ctx, &bindings);
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
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["api".into(), "api::schema".into(), "server".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/server.rs"))
            .module_paths(&mp)
            .current_module_path("server")
            .build();
        let deps = parse_file_dependencies(
            &syn::parse_file(source).unwrap(),
            &ctx,
            EdgeContext::production(),
        );
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
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["api".into(), "api::adapter".into(), "dispatch".into()]),
        )]
        .into_iter()
        .collect();
        let mut root_info = ModuleExportInfo::default();
        root_info.private_definitions.insert("Config".to_string());
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([(String::new(), root_info)]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/api/adapter.rs"))
            .module_paths(&mp)
            .current_module_path("api::adapter")
            .reexport_map(&map)
            .build();
        let deps = parse_file_dependencies(
            &syn::parse_file(source).unwrap(),
            &ctx,
            EdgeContext::production(),
        );
        assert!(
            deps.iter().any(|d| d.target_module == "dispatch"
                && d.target_item.as_deref() == Some("DispatchAdapter")),
            "the path names `DispatchAdapter` in `dispatch`: {deps:?}"
        );
    }

    /// A glob brings in the modules its module binds by `use`: by `pub use`,
    /// and by a private `use` only where the file sits below that module.
    #[rstest]
    #[case::private_use_of_an_ancestor("render::writer", "render", false, true)]
    #[case::pub_use("other", "render", true, true)]
    #[case::private_use_seen_from_outside("other", "render", false, false)]
    fn test_path_through_a_module_a_glob_brings_by_use_resolves(
        #[case] current_module: &str,
        #[case] glob_module: &str,
        #[case] public: bool,
        #[case] resolves: bool,
    ) {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from([
                "render".into(),
                "render::writer".into(),
                "other".into(),
                "shared".into(),
                "shared::flow".into(),
            ]),
        )]
        .into_iter()
        .collect();
        let binding = ReExportTarget {
            crate_name: "my_crate".to_string(),
            module: String::new(),
            original_name: "shared".to_string(),
        };
        let mut render_info = ModuleExportInfo::default();
        if public {
            render_info
                .explicit_reexports
                .insert("shared".to_string(), binding);
        } else {
            render_info
                .private_uses
                .insert("shared".to_string(), binding);
        }
        let map: ReExportMap = [(
            "my_crate".to_string(),
            HashMap::from([(glob_module.to_string(), render_info)]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/consumer.rs"))
            .module_paths(&mp)
            .current_module_path(current_module)
            .reexport_map(&map)
            .build();
        let source =
            format!("use crate::{glob_module}::*;\nfn f() {{\n    use shared::flow::Exit;\n}}");
        let deps = parse_file_dependencies(
            &syn::parse_file(&source).unwrap(),
            &ctx,
            EdgeContext::production(),
        );
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
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["value".into(), "value::record".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/value/mod.rs"))
            .module_paths(&mp)
            .current_module_path("value")
            .build();
        let deps = parse_file_dependencies(
            &syn::parse_file("pub use record::Record;\npub enum Value { Record(Record) }").unwrap(),
            &ctx,
            EdgeContext::production(),
        );
        let record: Vec<_> = deps
            .iter()
            .filter(|d| d.target_item.as_deref() == Some("Record"))
            .collect();
        assert_eq!(record.len(), 1, "deps: {deps:?}");
        assert_eq!(record[0].target_module, "value::record");
        assert!(!record[0].via_reexport, "the enum uses `Record`: {deps:?}");
    }
}

/// `use path::{self, ...}` imports the module itself — the `self` segment is
/// not an item name.
mod use_self_tests {
    use super::*;

    #[test]
    fn test_group_self_imports_module_without_item() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["auxil".into(), "auxil::dxgi".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/dx12/mod.rs"))
            .module_paths(&mp)
            .build();
        let uses = parse_test_uses("use crate::auxil::{self, dxgi::Factory};");
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());

        let auxil = deps
            .iter()
            .find(|d| d.target_module == "auxil")
            .expect("auxil dep should exist");
        assert_eq!(
            auxil.target_item, None,
            "`self` names the module, not an item: {deps:?}"
        );

        let dxgi = deps
            .iter()
            .find(|d| d.target_module == "auxil::dxgi")
            .expect("dxgi dep should exist");
        assert_eq!(dxgi.target_item, Some("Factory".to_string()));
    }

    #[test]
    fn test_plain_self_import_has_no_item() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["auxil".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/dx12/mod.rs"))
            .module_paths(&mp)
            .build();
        let uses = parse_test_uses("use crate::auxil::self;");
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "should resolve to the module: {deps:?}");
        assert_eq!(deps[0].target_module, "auxil");
        assert_eq!(deps[0].target_item, None);
    }

    #[test]
    fn test_self_rename_keeps_module_as_source() {
        let mp: ModulePathMap = [("my_crate".to_string(), HashSet::from(["auxil".into()]))]
            .into_iter()
            .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/dx12/mod.rs"))
            .module_paths(&mp)
            .build();
        let uses = parse_test_uses("use crate::auxil::{self as aux};");
        let deps = parse_workspace_dependencies(&uses, &ctx, &FileBindings::default());
        assert_eq!(deps.len(), 1, "should resolve to the module: {deps:?}");
        assert_eq!(deps[0].target_module, "auxil");
        assert_eq!(deps[0].target_item, None);
    }

    #[test]
    fn test_self_module_alias_binds_module() {
        let mp: ModulePathMap = [(
            "my_crate".to_string(),
            HashSet::from(["auxil".into(), "auxil::dxgi".into()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/dx12/mod.rs"))
            .module_paths(&mp)
            .build();
        let bindings = bindings_of("use crate::auxil::{self, dxgi::Factory};", &ctx);
        assert_eq!(
            bindings.target(BindingRegions::ROOT, "auxil"),
            Some("crate::auxil"),
            "`self` binds the module name locally: {bindings:?}"
        );
    }
}

/// One use per occurrence, each with the category its position and the
/// definition give it.
mod use_category_tests {
    use super::*;
    use crate::model::{SymbolUse, UseCategory};
    use std::sync::LazyLock;

    fn definition(kind: DefKind) -> Definition {
        Definition { kind, line: 1 }
    }

    /// A crate with one module `vocab` holding one item of each kind.
    fn vocab_map() -> ReExportMap {
        let mut vocab = ModuleExportInfo::default();
        vocab
            .definitions
            .insert("Config".to_string(), definition(DefKind::Struct));
        vocab
            .definitions
            .insert("Mode".to_string(), definition(DefKind::Enum));
        vocab
            .definitions
            .insert("LIMIT".to_string(), definition(DefKind::Const));
        vocab
            .definitions
            .insert("TABLE".to_string(), definition(DefKind::Static));
        vocab
            .definitions
            .insert("build".to_string(), definition(DefKind::Fn));
        vocab
            .definitions
            .insert("Step".to_string(), definition(DefKind::Trait));
        vocab.associated.insert(
            "Mode".to_string(),
            AssociatedItems {
                variants: HashSet::from(["Fast".into(), "Slow".into()]),
                ..AssociatedItems::default()
            },
        );
        vocab.associated.insert(
            "Config".to_string(),
            AssociatedItems {
                fns: HashSet::from(["new".into()]),
                consts: HashSet::from(["MAX".into()]),
                ..AssociatedItems::default()
            },
        );
        vocab.associated.insert(
            "Step".to_string(),
            AssociatedItems {
                fns: HashSet::from(["run".into()]),
                ..AssociatedItems::default()
            },
        );
        [(
            "my_crate".to_string(),
            HashMap::from([("vocab".to_string(), vocab)]),
        )]
        .into_iter()
        .collect()
    }

    static VOCAB_MP: LazyLock<ModulePathMap> = LazyLock::new(|| {
        [("my_crate".to_string(), HashSet::from(["vocab".into()]))]
            .into_iter()
            .collect()
    });
    static VOCAB_MAP: LazyLock<ReExportMap> = LazyLock::new(vocab_map);

    fn deps_of(source: &str) -> Vec<DependencyRef> {
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .module_paths(&VOCAB_MP)
            .reexport_map(&VOCAB_MAP)
            .current_module_path("user")
            .build();
        parse_file_dependencies(
            &syn::parse_file(source).unwrap(),
            &ctx,
            EdgeContext::production(),
        )
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
    /// bin root that defines nothing, seen from a file of the bin tree.
    fn bin_tree_deps_of(source: &str) -> Vec<DependencyRef> {
        let mut lib_root = ModuleExportInfo::default();
        lib_root
            .definitions
            .insert("Shared".to_string(), definition(DefKind::Struct));
        let roots = vec![
            RootReExports {
                kind: EntryKind::Lib,
                modules: HashMap::from([(String::new(), lib_root)]),
            },
            RootReExports {
                kind: EntryKind::Bin,
                modules: HashMap::from([(String::new(), ModuleExportInfo::default())]),
            },
        ];
        let map: ReExportMap = [("my_crate".to_string(), roots)].into_iter().collect();
        let bin_view = map.for_root("my_crate", 1);
        let ws: WorkspacePackages = ["my_crate".to_string()].into_iter().collect();
        let exports: PackageExportMap = [(
            "my_crate".to_string(),
            HashSet::from(["Shared".to_string()]),
        )]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/banner.rs"))
            .workspace_crates(&ws)
            .crate_exports(&exports)
            .reexport_map(&bin_view)
            .current_module_path("banner")
            .build();
        parse_file_dependencies(
            &syn::parse_file(source).unwrap(),
            &ctx,
            EdgeContext::production(),
        )
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
                (Some("Config"), test.clone()),
                (Some("Mode"), test),
                (Some("*"), EdgeContext::production()),
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

    /// A call of a name the map does not know: a capitalised name is a
    /// tuple-struct or variant constructor, anything else a fn.
    #[test]
    fn a_call_of_an_unknown_name_goes_by_spelling() {
        let ws: WorkspacePackages = ["other".to_string()].into_iter().collect();
        let mp: ModulePathMap = [
            ("my_crate".to_string(), HashSet::from(["vocab".into()])),
            ("other".to_string(), HashSet::from(["types".into()])),
        ]
        .into_iter()
        .collect();
        let ctx = ResolutionContextBuilder::new(Path::new("src/user.rs"))
            .workspace_crates(&ws)
            .module_paths(&mp)
            .build();
        let source = "fn f() { other::types::Wrapper(1); other::types::make(); other::types::Kind::Variant(2); }";
        let deps = parse_file_dependencies(
            &syn::parse_file(source).unwrap(),
            &ctx,
            EdgeContext::production(),
        );
        assert_eq!(uses_of(&deps, "Wrapper"), [use_at(1, UseCategory::Types)]);
        assert_eq!(uses_of(&deps, "make"), [use_at(1, UseCategory::Fns)]);
        assert_eq!(uses_of(&deps, "Kind"), [use_at(1, UseCategory::Types)]);
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

    /// An associated item the map does not know: a call is behaviour, a read
    /// goes by the spelling of the name.
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
