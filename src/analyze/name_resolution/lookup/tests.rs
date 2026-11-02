use super::super::bindings::{ModuleExportInfo, RootReExports, Visibility};
use super::*;
use crate::model::{Definition, EdgeContext, EntryKind, TargetRoot};
use std::path::Path;

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

    /// A bin module that globs a module of its own package's lib sees no
    /// `pub(crate)` trait of the lib, which is another crate than the bin:
    /// the glob passes on what a descendant named through it.
    #[test]
    fn a_bin_glob_on_the_own_lib_brings_in_no_trait_of_the_lib_crate() {
        let definition = |kind| BindingKind::Definition(Definition { kind, line: 1 });
        let mut lib_module = ModuleExportInfo::default();
        lib_module.bind(
            "f".to_string(),
            NameBinding::new(definition(DefKind::Fn), Visibility::Public),
        );
        lib_module.bind(
            "T".to_string(),
            NameBinding::new(
                definition(DefKind::Trait),
                Visibility::Restricted(String::new()),
            ),
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
}
