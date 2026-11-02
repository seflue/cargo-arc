//! The context a path is resolved in: the binding table as each build of a
//! crate root sees it, the crate the path is written in, and the module
//! around it.

use super::super::packages::{
    ModulePathMap, PackageExportMap, WorkspacePackages, normalize_crate_name,
};
use super::bindings::{CrateModule, Meaning, NameTarget, ReExportMap, SeenBindings, Viewer};
use super::lookup::{NameLookup, PassedOn, Resolution};
use super::place::{AfterName, ExternPrelude, append_to_path};
use crate::model::{Definition, DependencyRef, TargetRoot, UsageKind};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

/// The map as each build of a crate root sees it: the build without `test`
/// and, where the root has one, its test build. Which of the two reads a
/// piece of code follows from the code's context alone.
#[derive(Clone, Default)]
pub(super) struct RootBuilds {
    production: ReExportMap,
    /// The map as the root's test build sees it, where the root has modules
    /// of its test build: the map of a file only test builds load.
    test_build: Option<ReExportMap>,
}

impl RootBuilds {
    /// The builds of the root `map` is seen from. A map seen from no root,
    /// or from one without modules of its test build, reads test code as
    /// built without `test`.
    pub(super) fn of(map: ReExportMap) -> Self {
        Self {
            test_build: map.test_build(),
            production: map,
        }
    }

    /// The map that reads code of `kind`: test code in the root's test
    /// build, where it has one, production code in the build without `test`.
    pub(super) fn reading(&self, kind: UsageKind) -> &ReExportMap {
        self.reading_first(kind).0
    }

    /// The map that reads code of `kind`, and the other build where the root
    /// has two.
    fn reading_first(&self, kind: UsageKind) -> (&ReExportMap, Option<&ReExportMap>) {
        match (&self.test_build, kind) {
            (Some(test_build), UsageKind::Test(_)) => (test_build, Some(&self.production)),
            _ => (&self.production, self.test_build.as_ref()),
        }
    }

    /// The definitions of `module` in `crate_name`, from both builds: a
    /// `#[cfg(test)]` item is in the test build only. The build that reads
    /// code of `kind` comes first, and of alternatives in one build the one
    /// read last.
    pub(super) fn definitions(
        &self,
        crate_name: &str,
        module: &str,
        kind: UsageKind,
    ) -> HashMap<String, Definition> {
        let (first, other) = self.reading_first(kind);
        let mut definitions = HashMap::new();
        for map in std::iter::once(first).chain(other) {
            let Some(info) = map.get(crate_name).and_then(|modules| modules.get(module)) else {
                continue;
            };
            for (name, binding) in info.all() {
                if let Some(definition) = binding.definition() {
                    definitions.entry(name.to_string()).or_insert(*definition);
                }
            }
        }
        definitions
    }
}

/// One crate as its code resolves names: the crate, the workspace around it,
/// the module tree, and the builds of the root whose files are read.
pub(super) struct CrateView<'a> {
    /// The crate's key in `module_paths`: a package key, or for an
    /// integration test the full path of its root module.
    pub(super) crate_name: &'a str,
    pub(super) workspace_crates: &'a WorkspacePackages,
    pub(super) module_paths: &'a ModulePathMap,
    pub(super) crate_exports: &'a PackageExportMap,
    /// Code-side crate name -> `package_id` for external crates visible to
    /// the crate. Populated from `crate_name_map[crate_name]`.
    pub(super) external_crate_names: &'a HashMap<String, String>,
    /// The workspace crates the extern prelude of the root whose files are
    /// read names, in every module of the crate.
    pub(super) extern_prelude: &'a ExternPrelude,
    pub(super) builds: RootBuilds,
    pub(super) modules_bound: ModulesBound,
}

/// The module each module of a crate binds under a name, as
/// `PathEnd::of_path` asks it, kept once it is looked up. Every path of a
/// module that starts with the same name asks the same question, and the
/// answer walks the module's globs.
#[derive(Default)]
pub(super) struct ModulesBound(RefCell<HashMap<BoundQuestion, Option<NameTarget>>>);

impl ModulesBound {
    /// The module `module` binds under `name` in the given build, from
    /// `look_up` the first time it is asked.
    pub(super) fn bound_or_look_up(
        &self,
        test_build: bool,
        module: &str,
        name: &str,
        after: AfterName,
        look_up: impl FnOnce() -> Option<NameTarget>,
    ) -> Option<NameTarget> {
        let key = (test_build, module.to_string(), name.to_string(), after);
        let cached = self.0.borrow().get(&key).cloned();
        cached.unwrap_or_else(|| {
            let named = look_up();
            self.0.borrow_mut().insert(key, named.clone());
            named
        })
    }
}

/// What [`ModulesBound`] is asked: whether the build is the test build, the
/// module, the name, and whether the path goes on after it.
type BoundQuestion = (bool, String, String, AfterName);

/// Invariant context for dependency resolution within a single source file,
/// in one build of its root.
pub(super) struct ResolutionContext<'a> {
    pub(super) crate_view: &'a CrateView<'a>,
    pub(super) source_file: &'a Path,
    pub(super) current_module_path: &'a str,
    reexport_map: &'a ReExportMap,
    /// Whether the code read is the root's test build.
    pub(super) test_build: bool,
}

impl<'a> ResolutionContext<'a> {
    /// The context of `source_file`, the file of `current_module_path` in
    /// the crate of `crate_view`, reading production code.
    pub(super) fn new(
        crate_view: &'a CrateView<'a>,
        source_file: &'a Path,
        current_module_path: &'a str,
    ) -> Self {
        Self {
            crate_view,
            source_file,
            current_module_path,
            reexport_map: crate_view.builds.reading(UsageKind::Production),
            test_build: false,
        }
    }

    /// The same file, read from `module`, an inline module in it.
    pub(super) fn in_module<'m>(&'m self, module: &'m str) -> ResolutionContext<'m> {
        ResolutionContext {
            current_module_path: module,
            ..*self
        }
    }

    /// The same file, reading code of `kind`.
    pub(super) fn reading(&self, kind: UsageKind) -> Self {
        Self {
            reexport_map: self.crate_view.builds.reading(kind),
            test_build: kind != UsageKind::Production,
            ..*self
        }
    }

    /// The viewer from this file's module, looking through `root`.
    fn viewer(&self, root: TargetRoot) -> Viewer<'a> {
        self.reexport_map.viewer(root, self.current_module_path)
    }

    /// The viewer from this file's module that reads what `dep` points at.
    pub(super) fn viewer_for(&self, dep: &DependencyRef) -> Viewer<'a> {
        self.viewer(dep.target_root)
    }

    /// The name lookup from this file's module, looking through `root`.
    pub(super) fn lookup(&self, root: TargetRoot) -> NameLookup<'a> {
        NameLookup {
            viewer: self.viewer(root),
            module_paths: self.crate_view.module_paths,
        }
    }

    /// The name lookup from this file's module that reads what `dep` points
    /// at.
    pub(super) fn lookup_for(&self, dep: &DependencyRef) -> NameLookup<'a> {
        self.lookup(dep.target_root)
    }

    /// Whether the module of this file has a child module `name` in the
    /// module tree.
    pub(super) fn has_child_module(&self, name: &str) -> bool {
        self.crate_view
            .module_paths
            .get_or_empty(&normalize_crate_name(self.crate_view.crate_name))
            .contains(&append_to_path(self.current_module_path, name))
    }

    /// The bindings of the module `dep` points at, as this file sees it.
    pub(super) fn target_bindings(&self, dep: &DependencyRef) -> Option<SeenBindings<'a>> {
        self.viewer_for(dep).bindings_in(CrateModule::of(dep))
    }

    /// [`NameLookup::follow`] from this file's module, through the root `dep`
    /// resolves against, for a reference that asks `meaning` of `dep`'s item.
    pub(super) fn resolve_reexport(&self, dep: &mut DependencyRef, meaning: Meaning) -> Resolution {
        self.lookup_for(dep).follow(dep, meaning)
    }

    /// [`ResolutionContext::resolve_reexport`], with what the chain passed
    /// through noted in `passed_on`. Whether the chain places a definer, so
    /// that `dep` produces an edge.
    pub(super) fn resolve_noting(
        &self,
        dep: &mut DependencyRef,
        meaning: Meaning,
        passed_on: &mut PassedOn,
    ) -> bool {
        let resolution = self.resolve_reexport(dep, meaning);
        let places_a_definer = !resolution.places_no_definer();
        passed_on.note(resolution, self.current_module_path, dep);
        places_a_definer
    }
}
