//! Pass 2: the dependencies of each file of one package, resolved against
//! the table pass 1 built.

use std::collections::HashMap;
use std::path::Path;

use super::super::packages::{
    ModulePathMap, ModulePaths, PackageExportMap, WorkspacePackages, normalize_crate_name,
};
use super::super::reexports::root_extern_prelude;
use super::super::use_parser::{
    CrateView, ExternPrelude, ModulesBound, PassedOn, ReExportMap, ResolutionContext, RootBuilds,
};
use super::dependencies::parse_file_dependencies;
use crate::model::{Definition, DependencyRef, EdgeContext, PackageInfo, TargetRoot, UsageKind};

/// The workspace as the code of each of its packages resolves names: what
/// pass 1 gathered for all packages at once.
pub(super) struct WorkspaceView<'w> {
    pub(super) workspace_crates: WorkspacePackages,
    pub(super) module_paths: &'w ModulePathMap,
    pub(super) crate_exports: PackageExportMap,
    pub(super) table: ReExportMap,
    pub(super) include_tests: bool,
    pub(super) backend: Backend,
}

/// The backend whose walk asks the resolver, and whose resolution it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Backend {
    Syn,
    /// The HIR walk reads one root, the package's first entry file where it
    /// has one, under the package's name as written, and takes each file's
    /// dependencies as resolved.
    Hir,
}

/// A crate root of a package: one of its entry files, or an integration
/// test.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Root(RootKind);

#[derive(Debug, Clone, Copy)]
enum RootKind {
    /// The index of the root's file in the package's `entry_files`.
    Entry(usize),
    /// The index of the integration test in the order
    /// [`PackageResolver::integration_test`] added them.
    IntegrationTest(usize),
}

impl Root {
    /// The root whose file is the `index`-th of the package's `entry_files`.
    /// Resolving as the HIR backend, a package has the one root `entry(0)`,
    /// with or without an entry file.
    pub(crate) fn entry(index: usize) -> Self {
        Self(RootKind::Entry(index))
    }
}

/// One file of a module, and the context of the code it holds: test code
/// for a file only test builds load.
pub(crate) struct FileInModule<'f> {
    pub(crate) file: &'f Path,
    /// The module's path within its crate, `""` for the crate root.
    pub(crate) module_path: &'f str,
    pub(crate) context: EdgeContext,
}

/// The dependencies of one file, before the descendants of its module are
/// read. [`PackageResolver::finish`] turns them into the file's
/// dependencies.
pub(crate) struct Pending {
    root: Root,
    module_path: String,
    dependencies: Vec<DependencyRef>,
}

/// The name resolution of one package, through all its roots.
pub(crate) struct PackageResolver<'w> {
    roots: Roots<'w>,
    /// The private bindings of ancestors the files read so far named
    /// something through.
    passed_on: PassedOn,
}

/// The roots of a package, with what a [`CrateView`] of each borrows.
struct Roots<'w> {
    workspace: &'w WorkspaceView<'w>,
    package: &'w PackageInfo,
    crate_name: String,
    external_crate_names: &'w HashMap<String, String>,
    entries: Vec<EntryRoot>,
    unrooted: Unrooted,
    integration_tests: Vec<IntegrationTestRoot>,
}

/// What a [`CrateView`] of an entry file's root owns.
struct EntryRoot {
    extern_prelude: ExternPrelude,
    builds: RootBuilds,
    modules_bound: ModulesBound,
}

/// The package seen from no root, as each integration test sees it.
struct Unrooted {
    extern_prelude: ExternPrelude,
    builds: RootBuilds,
}

/// What a [`CrateView`] of an integration test owns. Its extern prelude is
/// that of [`Roots::unrooted`].
struct IntegrationTestRoot {
    /// The full path of the test's root module, its key in `module_paths`.
    crate_name: String,
    /// The workspace's module paths with those of the test.
    module_paths: ModulePathMap,
    builds: RootBuilds,
    modules_bound: ModulesBound,
}

impl<'w> PackageResolver<'w> {
    pub(super) fn new(
        workspace: &'w WorkspaceView<'w>,
        package: &'w PackageInfo,
        external_crate_names: &'w HashMap<String, String>,
    ) -> Self {
        let (crate_name, entries) = match workspace.backend {
            Backend::Syn => {
                let crate_name = normalize_crate_name(&package.name);
                let entries = package
                    .entry_files
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| EntryRoot {
                        extern_prelude: root_extern_prelude(package, Some(entry)),
                        builds: RootBuilds::of(workspace.table.for_root(&crate_name, index)),
                        modules_bound: ModulesBound::default(),
                    })
                    .collect();
                (crate_name, entries)
            }
            Backend::Hir => {
                let entry = EntryRoot {
                    extern_prelude: root_extern_prelude(package, package.entry_files.first()),
                    builds: RootBuilds::of(ReExportMap::default()),
                    modules_bound: ModulesBound::default(),
                };
                (package.name.clone(), vec![entry])
            }
        };
        let unrooted = Unrooted {
            extern_prelude: root_extern_prelude(package, None),
            builds: RootBuilds::of(workspace.table.clone()),
        };
        Self {
            roots: Roots {
                workspace,
                package,
                crate_name,
                external_crate_names,
                entries,
                unrooted,
                integration_tests: Vec::new(),
            },
            passed_on: PassedOn::default(),
        }
    }

    /// The root of the integration test whose root module has the full path
    /// `test_crate` and declares the modules `modules`.
    pub(crate) fn integration_test(&mut self, test_crate: &str, modules: ModulePaths) -> Root {
        let roots = &mut self.roots;
        roots.integration_tests.push(IntegrationTestRoot {
            crate_name: test_crate.to_string(),
            module_paths: roots.workspace.module_paths.with(test_crate, modules),
            builds: roots.unrooted.builds.clone(),
            modules_bound: ModulesBound::default(),
        });
        Root(RootKind::IntegrationTest(roots.integration_tests.len() - 1))
    }

    /// Resolve the paths `syntax`, the source of `file` in `root`, writes.
    /// What their re-export chains pass through is noted for
    /// [`PackageResolver::finish`] of the modules above.
    pub(crate) fn dependencies(
        &mut self,
        root: Root,
        file: FileInModule,
        syntax: &syn::File,
    ) -> Pending {
        let source_file = file
            .file
            .strip_prefix(&self.roots.package.workspace_root)
            .map_or_else(|_| file.file.to_path_buf(), Path::to_path_buf);
        // The view owns the root's builds and the answers it keeps; both go
        // back to the root for the next file.
        let modules_bound = std::mem::take(self.roots.modules_bound_mut(root));
        let builds = std::mem::take(self.roots.builds_mut(root));
        let view = self.roots.view(root, builds, modules_bound);
        let res_ctx = ResolutionContext::new(&view, &source_file, file.module_path);
        let dependencies = parse_file_dependencies(
            syntax,
            &res_ctx,
            file.context,
            self.roots.workspace.include_tests,
            &mut self.passed_on,
        );
        let CrateView {
            builds,
            modules_bound,
            ..
        } = view;
        *self.roots.builds_mut(root) = builds;
        *self.roots.modules_bound_mut(root) = modules_bound;
        Pending {
            root,
            module_path: file.module_path.to_string(),
            dependencies,
        }
    }

    /// The dependencies of the file `pending` was read from. Call it once the
    /// files of all descendants of its module are read: a private `use` or
    /// glob a descendant names something through only passes the names on,
    /// which is no dependency of the module. Without test code included,
    /// only dependencies of production code remain. Resolving as the HIR
    /// backend, the dependencies stay as resolved.
    pub(crate) fn finish(&self, pending: Pending) -> Vec<DependencyRef> {
        let Pending {
            root,
            module_path,
            mut dependencies,
        } = pending;
        if self.roots.workspace.backend == Backend::Hir {
            return dependencies;
        }
        if !self.roots.workspace.include_tests {
            dependencies.retain(|dep| dep.context.kind == UsageKind::Production);
        }
        dependencies.retain(|dep| !self.only_passes_on(root, &module_path, dep));
        target_file_modules(&mut dependencies, self.roots.module_paths(root));
        let own_package = normalize_crate_name(self.roots.crate_name(root));
        dependencies.retain(|dep| !stays_in_its_file(dep, &own_package, &module_path));
        dependencies
    }

    /// The definitions of the module at `module_path` in `root`, for code of
    /// `kind`: the build that reads such code first, as
    /// [`RootBuilds::definitions`] orders them.
    pub(crate) fn definitions(
        &self,
        root: Root,
        module_path: &str,
        kind: UsageKind,
    ) -> HashMap<String, Definition> {
        self.roots
            .builds(root)
            .definitions(self.roots.crate_name(root), module_path, kind)
    }

    /// Whether `dep` of the module at `module_path` only passes a name on to
    /// a descendant, as [`PassedOn::passes_on`] decides in the build `dep`'s
    /// code is compiled in.
    fn only_passes_on(&self, root: Root, module_path: &str, dep: &DependencyRef) -> bool {
        self.passed_on.passes_on(
            self.roots.crate_name(root),
            module_path,
            dep,
            self.roots.builds(root).reading(dep.context.kind),
            self.roots.module_paths(root),
        )
    }
}

/// Point each dependency on a module declared inline at the module of its
/// file, the node that holds it, with the inline module as the item where the
/// path ends on it.
fn target_file_modules(dependencies: &mut [DependencyRef], module_paths: &ModulePathMap) {
    for dep in dependencies {
        let file_module = module_paths.file_module(&dep.target_package, &dep.target_module);
        let target_is_inline = file_module.len() < dep.target_module.len();
        if !target_is_inline {
            continue;
        }
        let file_module = file_module.to_string();
        if dep.target_item.is_none() {
            let module = dep.target_module.as_str();
            let name = module.rsplit_once("::").map_or(module, |(_, name)| name);
            dep.target_item = Some(name.to_string());
        }
        dep.target_module = file_module;
    }
}

/// Whether `dep`, a dependency of the module at `module_path` in
/// `own_package`, points back at that module, as `use super::*` in an inline
/// module does once [`target_file_modules`] moved it to its file. Such a path
/// depends on nothing outside the file.
fn stays_in_its_file(dep: &DependencyRef, own_package: &str, module_path: &str) -> bool {
    dep.target_root == TargetRoot::AsSeen
        && dep.target_package == own_package
        && dep.target_module == module_path
}

impl Roots<'_> {
    /// The crate's key in the module paths of `root`.
    fn crate_name(&self, root: Root) -> &str {
        match root.0 {
            RootKind::Entry(_) => &self.crate_name,
            RootKind::IntegrationTest(index) => &self.integration_tests[index].crate_name,
        }
    }

    fn module_paths(&self, root: Root) -> &ModulePathMap {
        match root.0 {
            RootKind::Entry(_) => self.workspace.module_paths,
            RootKind::IntegrationTest(index) => &self.integration_tests[index].module_paths,
        }
    }

    fn extern_prelude(&self, root: Root) -> &ExternPrelude {
        match root.0 {
            RootKind::Entry(index) => &self.entries[index].extern_prelude,
            RootKind::IntegrationTest(_) => &self.unrooted.extern_prelude,
        }
    }

    fn builds(&self, root: Root) -> &RootBuilds {
        match root.0 {
            RootKind::Entry(index) => &self.entries[index].builds,
            RootKind::IntegrationTest(index) => &self.integration_tests[index].builds,
        }
    }

    fn builds_mut(&mut self, root: Root) -> &mut RootBuilds {
        match root.0 {
            RootKind::Entry(index) => &mut self.entries[index].builds,
            RootKind::IntegrationTest(index) => &mut self.integration_tests[index].builds,
        }
    }

    fn modules_bound_mut(&mut self, root: Root) -> &mut ModulesBound {
        match root.0 {
            RootKind::Entry(index) => &mut self.entries[index].modules_bound,
            RootKind::IntegrationTest(index) => &mut self.integration_tests[index].modules_bound,
        }
    }

    /// The crate of `root` as its code resolves names, through `builds` and
    /// with the answers `modules_bound` keeps.
    fn view(&self, root: Root, builds: RootBuilds, modules_bound: ModulesBound) -> CrateView<'_> {
        CrateView {
            crate_name: self.crate_name(root),
            workspace_crates: &self.workspace.workspace_crates,
            module_paths: self.module_paths(root),
            crate_exports: &self.workspace.crate_exports,
            external_crate_names: self.external_crate_names,
            extern_prelude: self.extern_prelude(root),
            builds,
            modules_bound,
        }
    }
}
