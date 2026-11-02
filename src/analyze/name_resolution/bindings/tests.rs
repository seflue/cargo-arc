use super::*;
use crate::model::EdgeContext;
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

mod table_construction_tests {
    use super::*;

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

mod viewer_tests {
    use super::*;

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
}

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
