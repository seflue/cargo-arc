//! Syn-based use statement parsing for workspace dependency extraction.

use super::packages::{ModulePathMap, PackageExportMap, WorkspacePackages, normalize_crate_name};
use crate::model::{
    DefKind, Definition, DependencyRef, EdgeContext, EntryKind, SymbolUse, TargetRoot, TestKind,
    UsageKind, UseCategory,
};
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use syn::UseTree;
use syn::visit::Visit;

use super::mod_resolver::is_cfg_test;

// ---------------------------------------------------------------------------
// Re-export resolution types (moved from reexports.rs)
// ---------------------------------------------------------------------------

/// Check whether visibility qualifies as a re-export:
/// pub, pub(crate), pub(super) — but NOT private or pub(in path).
pub(crate) fn is_reexport_visibility(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(r) => {
            r.in_token.is_none() && (r.path.is_ident("crate") || r.path.is_ident("super"))
        }
        syn::Visibility::Inherited => false,
    }
}

/// Where a binding is visible, as rustc resolves a visibility: everywhere, or
/// in one module of the binding's crate and the modules below it. `pub(crate)`
/// restricts to the root `""`, a private item to its own module, `pub(super)`
/// to the parent and `pub(in path)` to the module the path names.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Visibility {
    Public,
    Restricted(String),
}

impl Visibility {
    /// The visibility `vis` gives a binding declared in `module_path`.
    pub(crate) fn of(vis: &syn::Visibility, module_path: &str) -> Self {
        match vis {
            syn::Visibility::Public(_) => Self::Public,
            syn::Visibility::Inherited => Self::Restricted(module_path.to_string()),
            syn::Visibility::Restricted(restricted) => {
                let mut module: Vec<&str> =
                    module_path.split("::").filter(|s| !s.is_empty()).collect();
                let segments: Vec<String> = restricted
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect();
                for (index, segment) in segments.iter().enumerate() {
                    match segment.as_str() {
                        "crate" if index == 0 => module.clear(),
                        "self" if index == 0 => {}
                        "super" => {
                            module.pop();
                        }
                        // A path without `crate`, `self` or `super` in front
                        // starts at the root (edition 2015).
                        name if index == 0 => {
                            module.clear();
                            module.push(name);
                        }
                        name => module.push(name),
                    }
                }
                Self::Restricted(module.join("::"))
            }
        }
    }
}

/// Where a re-exported symbol originally comes from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReExportTarget {
    /// Source crate, normalized. Usually the crate the target belongs to
    /// already, but a re-export or a private `use` may cross into another
    /// workspace crate.
    pub(crate) crate_name: String,
    /// Source module path (crate-relative, e.g. `"render::elements"`)
    pub(crate) module: String,
    /// Original name in the source module (differs from map key on rename)
    pub(crate) original_name: String,
}

/// A glob `use` a module holds: the module it imports from and where the
/// names it brings in are visible.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlobSource {
    pub(crate) module: String,
    pub(crate) visibility: Visibility,
}

/// The items a type or trait carries with it, by name. They decide what
/// `Type::item` takes from the type: a variant builds a value, a fn is called,
/// a const is read.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct AssociatedItems {
    pub(crate) variants: HashSet<String>,
    pub(crate) fns: HashSet<String>,
    pub(crate) consts: HashSet<String>,
}

impl AssociatedItems {
    pub(crate) fn add_fn(&mut self, name: &syn::Ident) {
        self.fns.insert(name.to_string());
    }

    pub(crate) fn add_const(&mut self, name: &syn::Ident) {
        self.consts.insert(name.to_string());
    }

    /// Take in `other`'s items, so two `impl` blocks of one type add up.
    pub(crate) fn merge(&mut self, other: Self) {
        self.variants.extend(other.variants);
        self.fns.extend(other.fns);
        self.consts.extend(other.consts);
    }
}

/// One of the three namespaces of the Rust Reference, in which a binding
/// takes its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Namespace {
    /// Modules, structs, enums, traits, type aliases.
    Type,
    /// Functions, constants, statics.
    Value,
    /// Macros.
    Macro,
}

impl Namespace {
    /// The namespace an item of `kind` takes its name in. A tuple or unit
    /// struct also has a constructor in the value namespace, which `kind`
    /// does not tell.
    fn of(kind: DefKind) -> Self {
        match kind {
            DefKind::Fn | DefKind::Const | DefKind::Static => Self::Value,
            DefKind::Struct | DefKind::Enum | DefKind::Trait | DefKind::Type => Self::Type,
            DefKind::Macro => Self::Macro,
        }
    }
}

/// What a binding binds a name to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BindingKind {
    /// The module's own definition. In the root, every module of the crate
    /// can name a private item through `crate::`.
    Definition(Definition),
    /// A `pub use`, `pub(crate) use` or `pub(super) use`.
    ReExport(ReExportTarget),
    /// A private, `pub(self)` or `pub(in …)` `use`. Rust makes it visible to
    /// descendant modules, which may then name the symbol through this module
    /// even though it defines nothing. Resolving through it attributes the
    /// edge to the definer, not to this ancestor.
    PrivateUse(ReExportTarget),
    /// A private `use` of a path the resolution chain cannot place: an
    /// external crate, invisible while re-exports are collected. Nothing here
    /// names a definer, so a descendant's reference through this module
    /// produces no edge rather than staying on the ancestor.
    Elsewhere,
    /// A child module declared here. A glob or a path from where the module
    /// is not visible does not reach it.
    Module,
}

impl BindingKind {
    /// The place of the kind in the order of [`ModuleBindings::named`].
    fn rank(&self) -> u8 {
        match self {
            Self::Definition(_) => 0,
            Self::ReExport(_) => 1,
            Self::PrivateUse(_) => 2,
            Self::Elsewhere => 3,
            Self::Module => 4,
        }
    }
}

/// One binding of a name in a module: what the name stands for there, where
/// it is visible, and in which builds of the map that holds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NameBinding {
    pub(crate) kind: BindingKind,
    pub(crate) visibility: Visibility,
    pub(crate) builds: Brought,
}

impl NameBinding {
    /// A binding of `kind` with `visibility`, in every build of the map.
    #[cfg(test)]
    pub(crate) fn new(kind: BindingKind, visibility: Visibility) -> Self {
        Self {
            kind,
            visibility,
            builds: Brought::InEveryBuild,
        }
    }

    /// The namespace the binding takes its name in. `None` for a `use`, which
    /// takes the name in every namespace its target has, unknown until the
    /// target resolves.
    fn namespace(&self) -> Option<Namespace> {
        match &self.kind {
            BindingKind::Definition(definition) => Some(Namespace::of(definition.kind)),
            BindingKind::Module => Some(Namespace::Type),
            BindingKind::ReExport(_) | BindingKind::PrivateUse(_) | BindingKind::Elsewhere => None,
        }
    }

    /// The definition the binding names, where it is one.
    pub(crate) fn definition(&self) -> Option<&Definition> {
        match &self.kind {
            BindingKind::Definition(definition) => Some(definition),
            _ => None,
        }
    }

    /// The target of a `use`, marked `true` for a private one.
    pub(crate) fn use_target(&self) -> Option<(&ReExportTarget, bool)> {
        match &self.kind {
            BindingKind::ReExport(target) => Some((target, false)),
            BindingKind::PrivateUse(target) => Some((target, true)),
            BindingKind::Definition(_) | BindingKind::Elsewhere | BindingKind::Module => None,
        }
    }

    /// The target of a `pub use`.
    fn reexported(&self) -> Option<&ReExportTarget> {
        match &self.kind {
            BindingKind::ReExport(target) => Some(target),
            _ => None,
        }
    }
}

/// Export and re-export information for a single module, in one build: what
/// a `#[cfg]` keeps out of the build is not here.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ModuleExportInfo {
    /// The bindings of each name, in the order of [`ModuleBindings::named`].
    /// Alternatives under `cfg`, in one file or in alternative files, and
    /// bindings in different namespaces are each a binding of their own.
    bindings: HashMap<String, Vec<NameBinding>>,
    /// The associated items of the types and traits defined here, by the
    /// defined name. An `impl` block in another file of the crate lands here
    /// too, on the module that defines the type.
    pub(crate) associated: HashMap<String, AssociatedItems>,
    /// The globs of the module, in the order of [`ModuleBindings::globs`].
    globs: Vec<GlobBinding>,
}

/// A glob `use` as a module holds it: how far its names reach, the workspace
/// crate of the module it imports from where that is not the holder's own,
/// and the module there.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlobBinding {
    pub(crate) reach: GlobReach,
    pub(crate) foreign_crate: Option<String>,
    pub(crate) source: GlobSource,
}

impl GlobBinding {
    /// The place of the glob in the order of [`ModuleBindings::globs`].
    fn rank(&self) -> u8 {
        let reach = match self.reach {
            GlobReach::ReExport => 0,
            GlobReach::Private => 2,
        };
        reach + u8::from(self.foreign_crate.is_some())
    }
}

impl ModuleExportInfo {
    /// Whether the module holds nothing a resolution reads. A public child
    /// module is visible wherever the module is, as it is when the module
    /// holds no entry.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        let public_module = |binding: &NameBinding| {
            binding.kind == BindingKind::Module && binding.visibility == Visibility::Public
        };
        self.bindings.values().flatten().all(public_module)
            && self.associated.is_empty()
            && self.globs.is_empty()
    }

    /// Hold `glob`, read after the globs the module already holds.
    pub(crate) fn hold_glob(&mut self, glob: GlobBinding) {
        let rank = glob.rank();
        let at = self
            .globs
            .iter()
            .position(|held| held.rank() > rank)
            .unwrap_or(self.globs.len());
        self.globs.insert(at, glob);
    }

    /// Bind `name` by `binding`, read after the bindings the module already
    /// holds. A binding equal to one of them adds nothing.
    pub(crate) fn bind(&mut self, name: String, binding: NameBinding) {
        let bindings = self.bindings.entry(name).or_default();
        if bindings.contains(&binding) {
            return;
        }
        let rank = binding.kind.rank();
        let at = bindings
            .iter()
            .position(|known| known.kind.rank() >= rank)
            .unwrap_or(bindings.len());
        bindings.insert(at, binding);
    }

    /// Take in what another file of the same module holds (an alternative
    /// under `cfg`), read after this one: the bindings of both.
    pub(crate) fn merge(&mut self, other: Self) {
        for (name, bindings) in other.bindings {
            // Read oldest first, so each lands before those read before it.
            for binding in bindings.into_iter().rev() {
                self.bind(name.clone(), binding);
            }
        }
        for (name, items) in other.associated {
            self.associated.entry(name).or_default().merge(items);
        }
        for glob in other.globs {
            self.hold_glob(glob);
        }
    }

    /// The target of each `use` binding, with its name and its place among
    /// the bindings of the name.
    fn use_targets(&self) -> impl Iterator<Item = (&str, usize, &ReExportTarget)> {
        self.bindings.iter().flat_map(|(name, bindings)| {
            bindings
                .iter()
                .enumerate()
                .filter_map(move |(index, binding)| {
                    let (target, _) = binding.use_target()?;
                    Some((name.as_str(), index, target))
                })
        })
    }
}

/// Whether a glob's names reach outside its module, as a `pub use *`,
/// `pub(crate)` and `pub(super)` included, or only its descendants, as a
/// private, `pub(self)` or `pub(in …)` `use *`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobReach {
    ReExport,
    Private,
}

/// A glob `use` a module holds, by the crate of the module it imports from.
#[derive(Debug, Clone, Copy)]
struct GlobImport<'a> {
    crate_name: &'a str,
    source: &'a GlobSource,
    reach: GlobReach,
    /// Whether the source is in another workspace crate than the holder.
    foreign: bool,
}

impl<'a> GlobImport<'a> {
    /// The module the glob imports from.
    fn source_module(self) -> CrateModule<'a> {
        CrateModule {
            crate_name: self.crate_name,
            path: &self.source.module,
        }
    }
}

/// The bindings of one module of one crate, as a map shows the module, seen
/// or not. Name resolution reads them through [`SeenBindings`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModuleBindings<'a> {
    crate_name: &'a str,
    info: &'a ModuleExportInfo,
}

impl<'a> ModuleBindings<'a> {
    /// The bindings `info`, a module of `crate_name`, holds.
    pub(crate) fn new(crate_name: &'a str, info: &'a ModuleExportInfo) -> Self {
        Self { crate_name, info }
    }

    /// The bindings of `name`: definitions, `pub use` items, private `use`
    /// items, `use` items of something unplaced, child modules, in this
    /// order. Of one kind, the binding read last comes first, so a caller
    /// that takes one of several alternatives takes the one read last. The
    /// map is read once; a caller that asks several things of the bindings
    /// clones the iterator.
    pub(crate) fn named(
        self,
        name: &str,
    ) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.info.bindings.get(name).into_iter().flatten()
    }

    /// Every binding of every name, grouped by kind in the order of
    /// [`ModuleBindings::named`].
    pub(crate) fn all(self) -> impl Iterator<Item = (&'a str, &'a NameBinding)> {
        let bindings = &self.info.bindings;
        (0..=BindingKind::Module.rank()).flat_map(move |rank| {
            bindings.iter().flat_map(move |(name, bindings)| {
                bindings
                    .iter()
                    .filter(move |binding| binding.kind.rank() == rank)
                    .map(move |binding| (name.as_str(), binding))
            })
        })
    }

    /// The globs of the module: `pub use` globs of its own crate, of other
    /// workspace crates, then private globs of its own crate and of others.
    fn globs(self) -> impl Iterator<Item = GlobImport<'a>> {
        self.info.globs.iter().map(move |glob| GlobImport {
            crate_name: glob.foreign_crate.as_deref().unwrap_or(self.crate_name),
            source: &glob.source,
            reach: glob.reach,
            foreign: glob.foreign_crate.is_some(),
        })
    }

    /// The associated items of the type or trait the module defines as
    /// `name`.
    fn associated(self, name: &str) -> Option<&'a AssociatedItems> {
        self.info.associated.get(name)
    }
}

/// The bindings of one module as a [`Viewer`] sees them, the answer to "name
/// N in module M, seen by V". Each lookup gives what the viewer sees; what it
/// does not see comes only from [`SeenBindings::unseen`] and
/// [`SeenBindings::every`].
#[derive(Debug, Clone, Copy)]
struct SeenBindings<'a> {
    bindings: ModuleBindings<'a>,
    viewer: Viewer<'a>,
}

impl<'a> SeenBindings<'a> {
    fn sees(self, visibility: &Visibility) -> bool {
        self.viewer.sees(self.bindings.crate_name, visibility)
    }

    /// The bindings of `name` the viewer sees, in the order of
    /// [`ModuleBindings::named`].
    fn named(self, name: &str) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.bindings
            .named(name)
            .filter(move |binding| self.sees(&binding.visibility))
    }

    /// The bindings of `name` the viewer does not see, in the same order.
    fn unseen(self, name: &str) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.bindings
            .named(name)
            .filter(move |binding| !self.sees(&binding.visibility))
    }

    /// The bindings of `name`, seen or not, for a fact about the name that
    /// holds whoever looks: whether its alternatives differ, or what kind its
    /// definition is.
    fn every(self, name: &str) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.bindings.named(name)
    }

    /// Every binding of every name the viewer sees, in the order of
    /// [`ModuleBindings::all`].
    fn all(self) -> impl Iterator<Item = (&'a str, &'a NameBinding)> {
        self.bindings
            .all()
            .filter(move |(_, binding)| self.sees(&binding.visibility))
    }

    /// The globs of the module the viewer sees, in the order of
    /// [`ModuleBindings::globs`].
    fn globs(self) -> impl Iterator<Item = GlobImport<'a>> {
        self.bindings
            .globs()
            .filter(move |glob| self.sees(&glob.source.visibility))
    }

    /// The associated items of the type or trait the module defines as
    /// `name`.
    fn associated(self, name: &str) -> Option<&'a AssociatedItems> {
        self.bindings.associated(name)
    }
}

/// The builds of a map in which a module brings a name into a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Brought {
    /// Every build of the map.
    InEveryBuild,
    /// Only where a `#[cfg]` syn cannot decide holds, on the binding or on a
    /// `use` on the way.
    InSomeBuilds,
}

/// What one crate root's module tree exports: module path → export info.
/// Module paths are crate-relative (e.g. `"render"`, `"analyze::use_parser"`),
/// and `""` is the root file itself.
#[derive(Debug, Clone)]
pub(crate) struct RootReExports {
    pub(crate) kind: EntryKind,
    /// The modules the root's build without `test` loads.
    pub(crate) modules: HashMap<String, ModuleExportInfo>,
    /// The modules the root's test build loads, where they hold other files
    /// or other bindings than those of the build without `test`. Collected
    /// only when test code is read.
    pub(crate) test_build_modules: Option<HashMap<String, ModuleExportInfo>>,
}

impl RootReExports {
    /// The modules of each build the root holds, marked `true` for its test
    /// build.
    fn builds(&self) -> impl Iterator<Item = (bool, &HashMap<String, ModuleExportInfo>)> {
        std::iter::once((false, &self.modules)).chain(
            self.test_build_modules
                .iter()
                .map(|modules| (true, modules)),
        )
    }
}

/// Crate name → one [`RootReExports`] per entry file, in `entry_files` order.
///
/// A lib and a bin root of one package share the module tree but not their
/// root's exports. Another package sees only the lib root, the only one it can
/// depend on. The package of the file being resolved sees the root whose tree
/// holds the file, set with [`ReExportMap::for_root`]. Test code of that root
/// sees the root as its test build loads it, set with
/// [`ReExportMap::test_build`]; every other root and package it sees as built
/// without `test`, which is how its test build compiles them.
#[derive(Debug, Default, Clone)]
pub(crate) struct ReExportMap {
    packages: Arc<HashMap<String, Vec<RootReExports>>>,
    own_root: Option<(String, usize)>,
    /// Whether the own root is seen as its test build loads it.
    test_build: bool,
}

impl ReExportMap {
    /// The same map, seen from the `root`-th entry file of `crate_name`.
    pub(crate) fn for_root(&self, crate_name: &str, root: usize) -> Self {
        Self {
            packages: Arc::clone(&self.packages),
            own_root: Some((crate_name.to_string(), root)),
            test_build: false,
        }
    }

    /// The own root as a path through a package's name sees it: kept where it
    /// is its package's lib root, else none. Each package then shows its lib
    /// root; the lib that names its own package sees itself as this map does,
    /// its `pub(crate)` items and its test build included.
    fn lib_view_root(&self) -> Option<&(String, usize)> {
        self.own_root.as_ref().filter(|_| self.own_root_is_lib())
    }

    /// The viewer from `module` of the root this map is seen from, looking
    /// through `root`: [`TargetRoot::Lib`] sees every package's lib root, and
    /// the own root only where it is that lib.
    fn viewer<'a>(&'a self, root: TargetRoot, module: &'a str) -> Viewer<'a> {
        let own_root = match root {
            TargetRoot::AsSeen => self.own_root.as_ref(),
            TargetRoot::Lib => self.lib_view_root(),
        };
        Viewer {
            packages: &self.packages,
            own_root,
            test_build: self.test_build && own_root.is_some(),
            module: Some(module),
        }
    }

    /// The viewer from `module` that reads what `dep` points at, through the
    /// root `dep` resolves against.
    fn viewer_for<'a>(&'a self, dep: &DependencyRef, module: &'a str) -> Viewer<'a> {
        self.viewer(dep.target_root, module)
    }

    /// The same map, seen from the test build of the own root, or `None`
    /// where the root has no modules of its test build: they are then those
    /// of its build without `test`, or test code is not read.
    pub(crate) fn test_build(&self) -> Option<Self> {
        let (own, index) = self.own_root.as_ref()?;
        self.packages
            .get(own)?
            .get(*index)?
            .test_build_modules
            .as_ref()?;
        Some(Self {
            packages: Arc::clone(&self.packages),
            own_root: self.own_root.clone(),
            test_build: true,
        })
    }

    /// Point each `use` of another workspace crate whose target holds the path
    /// as written, where the empty map of collection left it, at what that
    /// path reaches through the crate's root. A path whose first name the root
    /// binds nothing under names that name in the root. Rounds repeat while
    /// one settles a path, since a path may run through a root binding that is
    /// itself written.
    pub(crate) fn settle_written_paths(mut self, module_paths: &ModulePathMap) -> Self {
        loop {
            let viewer = self.viewer_from_outside();
            let reached: Vec<_> = self
                .written_targets(module_paths)
                .filter_map(|(place, target)| {
                    Some((place, reached_target(viewer, target, module_paths)?))
                })
                .collect();
            if reached.is_empty() {
                break;
            }
            self.rebind(reached);
        }
        let unreached: Vec<_> = self
            .written_targets(module_paths)
            .map(|(place, target)| (place, root_name_of(target)))
            .collect();
        self.rebind(unreached);
        self
    }

    /// The viewer from outside every package: it sees each package's lib root,
    /// built without `test`, and only what is public there.
    fn viewer_from_outside(&self) -> Viewer<'_> {
        Viewer {
            packages: &self.packages,
            own_root: None,
            test_build: false,
            module: None,
        }
    }

    /// Each `use` binding whose target, in another workspace crate than its
    /// holder's, lies in no module of that crate: the path as written.
    fn written_targets<'a>(
        &'a self,
        module_paths: &'a ModulePathMap,
    ) -> impl Iterator<Item = (BindingPlace, &'a ReExportTarget)> + 'a {
        self.use_targets().filter(move |(place, target)| {
            target.crate_name != place.package
                && !target.module.is_empty()
                && !module_paths
                    .get_or_empty(&target.crate_name)
                    .contains(&target.module)
        })
    }

    /// Each `use` binding of the map, in every root and build, with its place.
    fn use_targets(&self) -> impl Iterator<Item = (BindingPlace, &ReExportTarget)> {
        let builds = self.packages.iter().flat_map(|(package, roots)| {
            roots.iter().enumerate().flat_map(move |(root, exports)| {
                exports
                    .builds()
                    .map(move |(test_build, modules)| (package, root, test_build, modules))
            })
        });
        builds.flat_map(|(package, root, test_build, modules)| {
            modules.iter().flat_map(move |(module, info)| {
                info.use_targets().map(move |(name, index, target)| {
                    let place = BindingPlace {
                        package: package.clone(),
                        root,
                        test_build,
                        module: module.clone(),
                        name: name.to_string(),
                        index,
                    };
                    (place, target)
                })
            })
        })
    }

    /// Replace the target of the `use` binding at each place.
    fn rebind(&mut self, targets: Vec<(BindingPlace, ReExportTarget)>) {
        let packages = Arc::make_mut(&mut self.packages);
        for (place, target) in targets {
            let binding = packages
                .get_mut(&place.package)
                .and_then(|roots| roots.get_mut(place.root))
                .and_then(|exports| {
                    if place.test_build {
                        exports.test_build_modules.as_mut()
                    } else {
                        Some(&mut exports.modules)
                    }
                })
                .and_then(|modules| modules.get_mut(&place.module))
                .and_then(|info| info.bindings.get_mut(&place.name))
                .and_then(|bindings| bindings.get_mut(place.index));
            if let Some(NameBinding {
                kind: BindingKind::ReExport(held) | BindingKind::PrivateUse(held),
                ..
            }) = binding
            {
                *held = target;
            }
        }
    }

    /// Whether the own root is seen as its test build loads it.
    pub(crate) fn is_test_build(&self) -> bool {
        self.test_build
    }

    /// The export info of `crate_name`'s modules, as this map sees them.
    pub(crate) fn get(&self, crate_name: &str) -> Option<&HashMap<String, ModuleExportInfo>> {
        self.viewer(TargetRoot::AsSeen, "")
            .modules(crate_name)
            .map(|(_, modules)| modules)
    }

    /// Whether the root this map is seen from is its package's lib root.
    fn own_root_is_lib(&self) -> bool {
        self.own_root.as_ref().is_some_and(|(own, index)| {
            self.packages
                .get(own)
                .and_then(|roots| roots.get(*index))
                .is_some_and(|root| root.kind == EntryKind::Lib)
        })
    }
}

/// Where a binding sits in a [`ReExportMap`]: package, root, build, module,
/// name, and place among the bindings of the name.
struct BindingPlace {
    package: String,
    root: usize,
    test_build: bool,
    module: String,
    name: String,
    index: usize,
}

/// The target the path `target` holds as written reaches from the root of
/// its crate, as `viewer` sees it: each name but the last carries the path
/// into the module it stands for, and the name that stands for no module
/// ends it, as an item whose associated item the rest names. `None` where
/// the root binds nothing under the first name.
fn reached_target(
    viewer: Viewer,
    target: &ReExportTarget,
    module_paths: &ModulePathMap,
) -> Option<ReExportTarget> {
    let written: Vec<&str> = target
        .module
        .split("::")
        .chain(std::iter::once(target.original_name.as_str()))
        .collect();
    let (last, through) = written.split_last()?;
    let mut crate_name = target.crate_name.clone();
    let mut module = String::new();
    for (index, name) in through.iter().enumerate() {
        let holder = CrateModule {
            crate_name: &crate_name,
            path: &module,
        };
        match name_in_module(viewer, holder, name, module_paths, true) {
            Some(NameTarget::Module {
                crate_name: reached_crate,
                path,
            }) => {
                crate_name = reached_crate;
                module = path;
            }
            None if index == 0 => return None,
            Some(NameTarget::Item) | None => return Some(target_in(crate_name, module, name)),
        }
    }
    Some(target_in(crate_name, module, last))
}

/// The target `name` in the module `module` of `crate_name`.
fn target_in(crate_name: String, module: String, name: &str) -> ReExportTarget {
    ReExportTarget {
        crate_name,
        module,
        original_name: name.to_string(),
    }
}

/// The first name of the path `target` holds as written, in the root of its
/// crate.
fn root_name_of(target: &ReExportTarget) -> ReExportTarget {
    let first = target.module.split("::").next().unwrap_or(&target.module);
    ReExportTarget {
        crate_name: target.crate_name.clone(),
        module: String::new(),
        original_name: first.to_string(),
    }
}

impl FromIterator<(String, Vec<RootReExports>)> for ReExportMap {
    fn from_iter<I: IntoIterator<Item = (String, Vec<RootReExports>)>>(iter: I) -> Self {
        Self {
            packages: Arc::new(iter.into_iter().collect()),
            own_root: None,
            test_build: false,
        }
    }
}

/// Packages with a lib root only.
impl FromIterator<(String, HashMap<String, ModuleExportInfo>)> for ReExportMap {
    fn from_iter<I: IntoIterator<Item = (String, HashMap<String, ModuleExportInfo>)>>(
        iter: I,
    ) -> Self {
        iter.into_iter()
            .map(|(crate_name, modules)| {
                let lib = RootReExports {
                    kind: EntryKind::Lib,
                    modules,
                    test_build_modules: None,
                };
                (crate_name, vec![lib])
            })
            .collect()
    }
}

/// Who looks a name up: a module of the crate root and build a
/// [`ReExportMap`] is seen from, looking through the root a reference
/// resolves against. Which root of a package it reads a module from and which
/// bindings it sees both follow from that, so a lookup asks the viewer and no
/// caller picks a root or a visibility check of its own.
#[derive(Debug, Clone, Copy)]
struct Viewer<'a> {
    packages: &'a HashMap<String, Vec<RootReExports>>,
    /// The root seen from inside, by package and entry-file index: its
    /// restricted bindings are visible below their module. Every other root
    /// shows only what is public, and every other package its lib root.
    own_root: Option<&'a (String, usize)>,
    /// Whether the own root is read as its test build loads it.
    test_build: bool,
    /// The module of the own root whose restricted bindings the viewer sees
    /// from, or `None` where it sees none of them.
    module: Option<&'a str>,
}

impl<'a> Viewer<'a> {
    /// The bindings of `module` as the viewer sees them, where the root the
    /// viewer reads its crate from holds the module.
    fn bindings_in(self, module: CrateModule) -> Option<SeenBindings<'a>> {
        let (crate_name, modules) = self.modules(module.crate_name)?;
        Some(SeenBindings {
            bindings: ModuleBindings::new(crate_name, modules.get(module.path)?),
            viewer: self,
        })
    }

    /// The viewer that sees only what both it and `holder` see, for the
    /// names a glob of `holder` brings in: a glob imports only what its
    /// holder sees. A holder in another crate than the own root's sees no
    /// restricted binding of the own root.
    fn through(self, holder: CrateModule) -> Self {
        let in_own_crate = self
            .own_root
            .is_some_and(|(own, _)| own == holder.crate_name);
        Self {
            module: self
                .module
                .filter(|_| in_own_crate)
                .map(|module| common_ancestor(module, holder.path)),
            ..self
        }
    }

    /// The modules of `crate_name`, as the viewer reads them: the own root,
    /// in its test build where the viewer reads that, or another package's
    /// lib root.
    fn modules(
        self,
        crate_name: &str,
    ) -> Option<(&'a String, &'a HashMap<String, ModuleExportInfo>)> {
        let (crate_name, roots) = self.packages.get_key_value(crate_name)?;
        let modules = match self.own_root {
            Some((own, index)) if own == crate_name => {
                let root = roots.get(*index)?;
                let test_build_modules =
                    root.test_build_modules.as_ref().filter(|_| self.test_build);
                test_build_modules.unwrap_or(&root.modules)
            }
            _ => {
                &roots
                    .iter()
                    .find(|root| root.kind == EntryKind::Lib)?
                    .modules
            }
        };
        Some((crate_name, modules))
    }

    /// Whether the viewer sees a binding with `visibility` in `crate_name`. A
    /// restricted binding is visible from below its module in the own root
    /// only: another package, another root of the same package and a viewer
    /// outside every package see what is public.
    fn sees(self, crate_name: &str, visibility: &Visibility) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Restricted(ancestor) => {
                self.own_root.is_some_and(|(own, _)| own == crate_name)
                    && self
                        .module
                        .is_some_and(|module| is_descendant(module, ancestor))
            }
        }
    }

    /// The viewer with which a resolution that reaches `crate_name` reads on.
    /// A crate other than the own root's is compiled against the own root
    /// built without `test`, so from there the way reads that build.
    fn reaching(self, crate_name: &str) -> Self {
        Self {
            test_build: self.test_build && self.own_root.is_none_or(|(own, _)| own == crate_name),
            ..self
        }
    }
}

/// A module holding a private `use` or glob, by crate and module path.
type Holder = (String, String);

/// A module of a workspace crate, by crate name and module path, borrowed.
#[derive(Debug, Clone, Copy)]
struct CrateModule<'a> {
    crate_name: &'a str,
    path: &'a str,
}

impl<'a> CrateModule<'a> {
    /// The module `dep` points at.
    fn of(dep: &'a DependencyRef) -> Self {
        Self {
            crate_name: &dep.target_package,
            path: &dep.target_module,
        }
    }
}

/// Where a name is defined: crate, module path and the name there.
type DefinedAt = (String, String, String);

/// The private `use` items and globs a descendant named something through,
/// per module that holds them. Such a module passes the names on to its
/// descendants. Each note records whether production code named something
/// through it, since only that passes a name on for production code.
#[derive(Debug, Default)]
pub(crate) struct PassedOn {
    /// Holder and the module its glob imports from.
    globs: HashMap<(Holder, String), FromProduction>,
    /// Holder and where the name the descendant reached is defined.
    uses: HashMap<(Holder, DefinedAt), FromProduction>,
}

/// Whether production code, not only test code, named something through a
/// note's holder.
type FromProduction = bool;

/// What a re-export chain followed from a reference: whether it ends on a
/// definer syn can place, the name it ends on, and the bindings of other
/// modules it passed through. Following the chain notes nothing; a caller
/// that keeps the reference notes the chain in [`PassedOn`].
#[derive(Debug)]
pub(crate) struct Resolution {
    ends_on_no_definer: bool,
    name: Option<String>,
    passed: Vec<PassedThrough>,
}

impl Resolution {
    /// Whether the chain reaches a name a private `use` bound to something
    /// outside the crate, or to targets syn cannot tell apart: the reference
    /// then produces no edge.
    pub(crate) fn places_no_definer(&self) -> bool {
        self.ends_on_no_definer
    }
}

/// A private binding a re-export chain passed through, by the module that
/// holds it.
#[derive(Debug, PartialEq)]
pub(crate) enum PassedThrough {
    /// A private `use` that binds the name the chain followed.
    PrivateUse(Holder),
    /// A private glob, with the module it imports from.
    PrivateGlob { holder: Holder, source: String },
}

impl PassedOn {
    /// Note what `resolution`, the chain of `dep` resolved from
    /// `importer_module`, passed through in modules other than
    /// `importer_module` itself: each private glob, and each private `use`
    /// where the chain places a definer, under the name `dep` now points at.
    pub(crate) fn note(
        &mut self,
        resolution: Resolution,
        importer_module: &str,
        dep: &DependencyRef,
    ) {
        let from_production = dep.context.kind == UsageKind::Production;
        for step in resolution.passed {
            match step {
                PassedThrough::PrivateGlob { holder, source } if holder.1 != importer_module => {
                    *self.globs.entry((holder, source)).or_default() |= from_production;
                }
                PassedThrough::PrivateUse(holder)
                    if holder.1 != importer_module && !resolution.ends_on_no_definer =>
                {
                    let Some(name) = &resolution.name else {
                        continue;
                    };
                    let defined_at = (
                        dep.target_package.clone(),
                        dep.target_module.clone(),
                        name.clone(),
                    );
                    *self.uses.entry((holder, defined_at)).or_default() |= from_production;
                }
                PassedThrough::PrivateGlob { .. } | PassedThrough::PrivateUse(_) => {}
            }
        }
    }

    /// Whether `dep`, a dependency of `holder` in `crate_name`, only passes a
    /// name on to a descendant that named something through it: the `*` of a
    /// private glob, which stands for no name `holder` uses itself, since a
    /// used name replaces the `*`, or a private `use` whose name nothing in
    /// `holder`'s file names. A trait import stays: its methods may be called
    /// without the name being written. So does a glob that may bring in a
    /// trait, as `reexport_map` tells. A production dependency passes on only
    /// what production code named through it.
    pub(crate) fn passes_on(
        &self,
        crate_name: &str,
        holder: &str,
        dep: &DependencyRef,
        reexport_map: &ReExportMap,
        module_paths: &ModulePathMap,
    ) -> bool {
        if dep.via_reexport {
            return false;
        }
        if is_glob(dep) {
            let noted = (
                (crate_name.to_string(), holder.to_string()),
                dep.target_module.clone(),
            );
            return dep.target_package == crate_name
                && covers(self.globs.get(&noted), dep)
                && !glob_may_bring_a_trait(
                    dep,
                    reexport_map.viewer_for(dep, holder),
                    module_paths,
                    &mut HashSet::new(),
                );
        }
        let holder = (crate_name.to_string(), holder.to_string());
        let Some(item) = &dep.target_item else {
            return false;
        };
        let noted = (
            holder,
            (
                dep.target_package.clone(),
                dep.target_module.clone(),
                item.clone(),
            ),
        );
        is_import_only(dep) && covers(self.uses.get(&noted), dep)
    }
}

/// Whether a note passes `dep` on: a production dependency only through a
/// note from production code, a test dependency through any.
fn covers(note: Option<&FromProduction>, dep: &DependencyRef) -> bool {
    note.is_some_and(|&from_production| {
        from_production || dep.context.kind != UsageKind::Production
    })
}

/// Whether the only use of `dep` is the one [`import_only_use`] derives for an
/// import nothing in the file names, and it is no trait imported for its
/// methods.
fn is_import_only(dep: &DependencyRef) -> bool {
    matches!(
        dep.uses.as_slice(),
        [only] if only.line == dep.line && !only.imported_for_methods
    )
}

/// Whether the glob `dep` brings a trait into the module `viewer` looks
/// from, or a name whose kind the map does not know at its definer. A module
/// the map does not hold counts as unknown, and so does a name the chain
/// follows out of the map or a `use` binds outside the crate. The glob brings
/// only the bindings `viewer` sees, through the root `dep` resolves against.
fn glob_may_bring_a_trait(
    dep: &DependencyRef,
    viewer: Viewer,
    module_paths: &ModulePathMap,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    let viewer = viewer.reaching(&dep.target_package);
    if !visited.insert((dep.target_package.clone(), dep.target_module.clone())) {
        return false;
    }
    let module = CrateModule::of(dep);
    let Some(bindings) = viewer.bindings_in(module) else {
        return true;
    };
    let defines_a_trait = bindings.all().any(|(_, binding)| {
        binding
            .definition()
            .is_some_and(|definition| definition.kind == DefKind::Trait)
    });
    let reexports_a_trait = bindings
        .all()
        .filter(|(_, binding)| binding.use_target().is_some())
        .any(|(name, _)| reexports_a_trait_or_unknown(dep, name, viewer, module_paths));
    let binds_elsewhere = bindings
        .all()
        .any(|(_, binding)| binding.kind == BindingKind::Elsewhere);
    let mut reexported_globs = bindings
        .globs()
        .filter(|glob| glob.reach == GlobReach::ReExport);
    defines_a_trait
        || reexports_a_trait
        || binds_elsewhere
        || reexported_globs.any(|glob| {
            let source = DependencyRef {
                target_package: glob.crate_name.to_string(),
                target_module: glob.source.module.clone(),
                ..dep.clone()
            };
            glob_may_bring_a_trait(&source, viewer.through(module), module_paths, visited)
        })
}

/// Whether `name`, re-exported by the module `dep` imports from, is a trait at
/// its definer, or the chain ends on no definition the map knows. A name that
/// ends on a module is no trait. Nothing in the holder `viewer` looks from
/// asks a meaning of the name, so the chain follows any.
fn reexports_a_trait_or_unknown(
    dep: &DependencyRef,
    name: &str,
    viewer: Viewer,
    module_paths: &ModulePathMap,
) -> bool {
    let mut named = DependencyRef {
        target_item: Some(name.to_string()),
        ..dep.clone()
    };
    if follow_reexports(&mut named, viewer, module_paths, Meaning::Any).places_no_definer() {
        return true;
    }
    let Some(item) = &named.target_item else {
        return false;
    };
    viewer
        .bindings_in(CrateModule::of(&named))
        .and_then(|bindings| bindings.every(item).find_map(NameBinding::definition))
        .is_none_or(|definition| definition.kind == DefKind::Trait)
}

/// Resolve re-exports in a [`DependencyRef`]: follow the re-export chain
/// until the original definition module is found, crossing into another
/// workspace crate where the chain does.
/// Modifies `dep.target_package` and `dep.target_module` in place; a no-op if
/// no re-export applies. The [`Resolution`] tells whether the chain instead
/// reaches a name a private `use` bound to something outside the crate (an
/// external crate), and which private `use` items and globs of other modules
/// it passed through. A `dep` on [`TargetRoot::Lib`] follows the chain from
/// its package's lib root. Whether a name binds a module, `module_paths`
/// tells; whether the reference means it, `meaning`.
fn resolve_reexport_as(
    dep: &mut DependencyRef,
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
    importer_module: &str,
    meaning: Meaning,
) -> Resolution {
    let viewer = reexport_map.viewer_for(dep, importer_module);
    follow_reexports(dep, viewer, module_paths, meaning)
}

/// The chain of [`resolve_reexport_as`], followed as `viewer` sees it.
fn follow_reexports(
    dep: &mut DependencyRef,
    mut viewer: Viewer,
    module_paths: &ModulePathMap,
    meaning: Meaning,
) -> Resolution {
    let mut lookup_name = dep.target_item.clone();
    let mut passed = Vec::new();
    let mut visited = HashSet::new();

    loop {
        viewer = viewer.reaching(&dep.target_package);
        if !visited.insert((dep.target_package.clone(), dep.target_module.clone())) {
            break;
        }
        let Some(bindings) = viewer.bindings_in(CrateModule::of(dep)) else {
            break;
        };
        let Some(item) = &lookup_name else {
            break;
        };

        let named = bindings.named(item);
        if named.clone().any(|binding| binding.definition().is_some()) {
            break;
        }

        // Tier 1: Explicit re-export.
        // Tier 1b: Private `use` binding, visible only to descendants of the
        // module that holds it, or of the module `pub(in …)` names, in the
        // same crate. A descendant naming the symbol through this ancestor
        // really depends on the definer, in the same crate or another
        // workspace crate.
        // Tier 1c: Bound elsewhere, visible only to descendants like Tier 1b.
        let binding = visible_binding(named);
        if places_no_definer(bindings, item, binding) {
            return Resolution {
                ends_on_no_definer: true,
                name: lookup_name,
                passed,
            };
        }
        if let Some((target, private_use)) = binding.and_then(NameBinding::use_target) {
            if end_on_module(dep, target, viewer, module_paths, meaning) {
                break;
            }
            if private_use {
                let holder = (dep.target_package.clone(), dep.target_module.clone());
                passed.push(PassedThrough::PrivateUse(holder));
            }
            tracing::debug!(
                "re-export resolved: {} -> {} (via a binding in {})",
                dep.target_module,
                target.module,
                dep.target_module
            );
            dep.target_package = target.crate_name.clone();
            dep.target_module = target.module.clone();
            lookup_name = Some(target.original_name.clone());
            continue;
        }

        if follow_glob(dep, bindings, viewer, item, &visited, &mut passed) {
            continue;
        }

        break;
    }
    // A rename on the way leaves the written name behind: the symbol is the
    // one the module the chain ends on knows.
    if dep.target_item.is_some() {
        dep.target_item.clone_from(&lookup_name);
    }
    Resolution {
        ends_on_no_definer: false,
        name: lookup_name,
        passed,
    }
}

/// Point `dep` at the module a glob of `bindings`, the module `dep` points
/// at, takes `symbol` from for `viewer`: a `pub use` glob of this
/// crate first, then of another workspace crate, then a private glob, which
/// reaches descendants only. A private glob of this crate is pushed onto
/// `passed`. Returns whether a glob brings the symbol.
fn follow_glob(
    dep: &mut DependencyRef,
    bindings: SeenBindings,
    viewer: Viewer,
    symbol: &str,
    visited: &HashSet<(String, String)>,
    passed: &mut Vec<PassedThrough>,
) -> bool {
    // Tier 2: Glob re-exports, of this crate first, then of another
    // workspace crate.
    let holder = CrateModule::of(dep);
    let globs = |reach: GlobReach, foreign: bool| {
        bindings
            .globs()
            .filter(move |glob| glob.reach == reach && glob.foreign == foreign)
    };
    let reexported = [false, true].into_iter().find_map(|foreign| {
        glob_bringing(
            viewer,
            holder,
            globs(GlobReach::ReExport, foreign),
            symbol,
            visited,
        )
    });
    if let Some(glob) = reexported {
        tracing::debug!(
            "re-export resolved: {} -> {} (via glob re-export in {})",
            dep.target_module,
            glob.source.module,
            dep.target_module
        );
        dep.target_package = glob.crate_name.to_string();
        dep.target_module.clone_from(&glob.source.module);
        return true;
    }

    // Tier 2b: Private glob, visible only to descendants of the module
    // that holds it, like Tier 1b. One of this crate is passed through.
    let private = [false, true].into_iter().find_map(|foreign| {
        glob_bringing(
            viewer,
            holder,
            globs(GlobReach::Private, foreign),
            symbol,
            &HashSet::new(),
        )
    });
    if let Some(glob) = private {
        if !glob.foreign {
            passed.push(PassedThrough::PrivateGlob {
                holder: (dep.target_package.clone(), dep.target_module.clone()),
                source: glob.source.module.clone(),
            });
        }
        dep.target_package = glob.crate_name.to_string();
        dep.target_module.clone_from(&glob.source.module);
        return true;
    }
    false
}

/// Whether a reference to `name` in the module of `bindings` names no definer
/// syn can place, and so produces no edge: private `use` items bind the name
/// to different targets and `binding` is one of them or missing, or, without
/// a `binding`, a `use` the viewer sees binds the name outside the crate. A
/// `binding` goes missing where its `use` is in builds the reference is not;
/// which of the other targets the reference has, the bindings do not tell.
fn places_no_definer(bindings: SeenBindings, name: &str, binding: Option<&NameBinding>) -> bool {
    match binding.and_then(NameBinding::use_target) {
        Some((_, private_use)) => private_use && private_targets_differ(bindings.every(name)),
        None => binds_to_differing_targets(bindings, name) || binds_elsewhere(bindings.named(name)),
    }
}

/// Whether the private `use` items among `named`, the bindings of one name,
/// bind it to different targets: alternatives under `cfg` in one file or in
/// alternative files.
fn private_targets_differ<'a>(named: impl Iterator<Item = &'a NameBinding>) -> bool {
    let mut targets = named.filter_map(|binding| match &binding.kind {
        BindingKind::PrivateUse(target) => Some(target),
        _ => None,
    });
    targets
        .next()
        .is_some_and(|first| targets.any(|target| target != first))
}

/// Whether private `use` items bind `name` in the module of `bindings` to
/// different targets, one of them seen by the viewer, whatever builds it is
/// in.
fn binds_to_differing_targets(bindings: SeenBindings, name: &str) -> bool {
    private_targets_differ(bindings.every(name))
        && bindings
            .named(name)
            .any(|binding| matches!(binding.kind, BindingKind::PrivateUse(_)))
}

/// Whether a `use` among `named`, the bindings of one name, binds it to
/// something the resolution chain cannot place.
fn binds_elsewhere<'a>(mut named: impl Iterator<Item = &'a NameBinding>) -> bool {
    named.any(|binding| binding.kind == BindingKind::Elsewhere)
}

/// The `use` binding among `named`, the bindings of one name a viewer sees:
/// a `pub use` first, then a private `use`.
fn visible_binding<'a>(
    mut named: impl Iterator<Item = &'a NameBinding>,
) -> Option<&'a NameBinding> {
    named.find(|binding| binding.use_target().is_some())
}

/// The first of `globs`, globs held by `holder` that `viewer` sees, whose
/// source exports `symbol` to `viewer` through `holder` without passing a
/// module in `passed`: a source that reaches it only through a module the
/// chain already passed leads back, not on.
fn glob_bringing<'a>(
    viewer: Viewer,
    holder: CrateModule,
    mut globs: impl Iterator<Item = GlobImport<'a>>,
    symbol: &str,
    passed: &HashSet<(String, String)>,
) -> Option<GlobImport<'a>> {
    globs.find(|glob| {
        module_exports_symbol(
            viewer.through(holder),
            glob.source_module(),
            symbol,
            &mut passed.clone(),
        )
        .is_some()
    })
}

/// Point `dep` at the module `target` binds, when it binds a module or a crate
/// root that `viewer` sees: the reference then names the module, not an item
/// in it. A reference whose `meaning` is no module goes on to an item of that
/// name beside the module, where its holder shows one.
fn end_on_module(
    dep: &mut DependencyRef,
    target: &ReExportTarget,
    viewer: Viewer,
    module_paths: &ModulePathMap,
    meaning: Meaning,
) -> bool {
    let item_meant = || {
        !meaning.admits_a_module()
            && holds_an_item_named(
                viewer,
                module_paths,
                CrateModule {
                    crate_name: &target.crate_name,
                    path: &target.module,
                },
                &target.original_name,
            )
    };
    let binds_module =
        binds_a_module(target, module_paths) && module_is_visible(viewer, target) && !item_meant();
    if binds_module {
        dep.target_package.clone_from(&target.crate_name);
        dep.target_module = append_to_path(&target.module, &target.original_name);
        dep.target_item = None;
    }
    binds_module
}

/// Whether `module` holds an item `name` that `viewer` sees: its own
/// definition, or a `use` of something that is no module. Beside a module of
/// that name, it is what a reference that does not mean the module names.
fn holds_an_item_named(
    viewer: Viewer,
    module_paths: &ModulePathMap,
    module: CrateModule,
    name: &str,
) -> bool {
    let Some(bindings) = viewer.bindings_in(module) else {
        return false;
    };
    let named = bindings.named(name);
    named.clone().any(|binding| binding.definition().is_some())
        || visible_binding(named)
            .and_then(NameBinding::use_target)
            .is_some_and(|(target, _)| !binds_a_module(target, module_paths))
}

/// Whether `target` binds a module of a workspace crate, or its root, as the
/// module tree has it.
fn binds_a_module(target: &ReExportTarget, module_paths: &ModulePathMap) -> bool {
    let module = append_to_path(&target.module, &target.original_name);
    if module.is_empty() {
        module_paths.contains_key(&target.crate_name)
    } else {
        module_paths
            .get_or_empty(&target.crate_name)
            .contains(&module)
    }
}

/// Whether `viewer` sees the module `target` binds, as the `mod` declarations
/// in its parent have it. A crate root is visible, and so is a module the map
/// holds no restricted declaration of.
fn module_is_visible(viewer: Viewer, target: &ReExportTarget) -> bool {
    let parent = CrateModule {
        crate_name: &target.crate_name,
        path: &target.module,
    };
    viewer
        .bindings_in(parent)
        .is_none_or(|parent| sees_the_module(parent, &target.original_name))
}

/// Whether the viewer of `bindings` sees a declaration of the child module
/// `name` among them, or there is none. A parent that declares only public
/// modules may be missing from the map.
fn sees_the_module(bindings: SeenBindings, name: &str) -> bool {
    let is_module = |binding: &NameBinding| binding.kind == BindingKind::Module;
    bindings.named(name).any(is_module) || !bindings.unseen(name).any(is_module)
}

/// Check whether `module` brings a symbol into the scope of the module
/// `viewer` looks from: by its own definition, a `use` of any visibility, a
/// name a `use` binds outside the crate, or a glob of any visibility,
/// followed into other workspace crates, each only where `viewer` sees it. A
/// glob brings in only what its holder sees, so the glob's source is read
/// through [`Viewer::through`] the module. Private `use` items that bind the
/// symbol to different targets bring it in every build: the module holds it
/// then in whichever, and a glob of the module does not go on to another. A
/// `use` under a `#[cfg]` syn cannot decide brings it in some builds only,
/// unless a glob of the module brings it in every one. A module in `visited`,
/// by crate and module path, brings nothing more. `None` where the module
/// does not bring the symbol.
fn module_exports_symbol(
    viewer: Viewer,
    module: CrateModule,
    symbol: &str,
    visited: &mut HashSet<(String, String)>,
) -> Option<Brought> {
    let crate_name = module.crate_name;
    let viewer = viewer.reaching(crate_name);
    if !visited.insert((crate_name.to_string(), module.path.to_string())) {
        return None;
    }
    let bindings = viewer.bindings_in(module)?;
    let named = bindings.named(symbol);
    if named.clone().any(|binding| binding.definition().is_some()) {
        return Some(Brought::InEveryBuild);
    }
    let binding = visible_binding(named.clone());
    let bound_in_every_build = binding
        .is_some_and(|binding| binding.builds == Brought::InEveryBuild)
        || binds_to_differing_targets(bindings, symbol)
        || binds_elsewhere(named);
    if bound_in_every_build {
        return Some(Brought::InEveryBuild);
    }
    let mut brought = binding.map(|_| Brought::InSomeBuilds);
    let local_globs = bindings.globs().filter(|glob| !glob.foreign);
    let foreign_globs = bindings.globs().filter(|glob| glob.foreign);
    for glob in local_globs.chain(foreign_globs) {
        let source = glob.source_module();
        match module_exports_symbol(viewer.through(module), source, symbol, visited) {
            Some(Brought::InEveryBuild) => return Some(Brought::InEveryBuild),
            Some(Brought::InSomeBuilds) => brought = Some(Brought::InSomeBuilds),
            None => {}
        }
    }
    brought
}

/// What a name in a module stands for, as a viewer sees it.
#[derive(Debug, Clone, PartialEq)]
enum NameTarget {
    /// A module of a workspace crate, to which a path carries its rest. Its
    /// path is `""` for the crate root.
    Module { crate_name: String, path: String },
    /// An item the module binds under the name, its own or through a `use`
    /// or a glob.
    Item,
}

/// Look up what `name` in `module` stands for, as `viewer` sees it. `None`
/// where the module binds nothing `viewer` sees under the name. The order is
/// a child module, a `use` that binds a module, the module's own items and
/// other `use` items, then its globs, so a binding of the module hides a
/// glob as in rustc. Where `path_goes_on` after the name, a `use` is followed
/// through further `use` items to the module it binds. There a definition
/// outside the type namespace stands for the name only where nothing else
/// does, since a path goes on only through a module or a type.
fn name_in_module(
    viewer: Viewer,
    module: CrateModule,
    name: &str,
    module_paths: &ModulePathMap,
    path_goes_on: bool,
) -> Option<NameTarget> {
    look_up_name(
        viewer,
        module,
        name,
        module_paths,
        path_goes_on,
        &mut HashSet::new(),
    )
}

/// [`name_in_module`], with the modules and names a chain of `use` items
/// already passed in `visited`.
fn look_up_name(
    viewer: Viewer,
    module: CrateModule,
    name: &str,
    module_paths: &ModulePathMap,
    path_goes_on: bool,
    visited: &mut HashSet<(String, String, String)>,
) -> Option<NameTarget> {
    let crate_name = module.crate_name;
    let viewer = viewer.reaching(crate_name);
    if !visited.insert((
        crate_name.to_string(),
        module.path.to_string(),
        name.to_string(),
    )) {
        return None;
    }
    let child = append_to_path(module.path, name);
    if module_paths.get_or_empty(crate_name).contains(&child) {
        return Some(NameTarget::Module {
            crate_name: crate_name.to_string(),
            path: child,
        });
    }
    let bindings = viewer.bindings_in(module)?;
    let named = bindings.named(name);
    let uses = named.clone().filter_map(NameBinding::use_target);
    for (target, _) in uses {
        if binds_a_module(target, module_paths) {
            return Some(NameTarget::Module {
                crate_name: target.crate_name.clone(),
                path: append_to_path(&target.module, &target.original_name),
            });
        }
        if path_goes_on {
            let bound = look_up_name(
                viewer,
                CrateModule {
                    crate_name: &target.crate_name,
                    path: &target.module,
                },
                &target.original_name,
                module_paths,
                path_goes_on,
                visited,
            );
            if let Some(module) = bound.filter(|bound| *bound != NameTarget::Item) {
                return Some(module);
            }
        }
    }
    let item_here = named.clone().any(|binding| match binding.kind {
        BindingKind::Definition(_) => !path_goes_on || binding.namespace() == Some(Namespace::Type),
        BindingKind::ReExport(_) | BindingKind::PrivateUse(_) | BindingKind::Elsewhere => true,
        BindingKind::Module => false,
    });
    if item_here {
        return Some(NameTarget::Item);
    }
    let glob_module = bindings.globs().find_map(|glob| {
        glob_child_module(
            viewer.through(module),
            glob.source_module(),
            name,
            module_paths,
            &mut HashSet::new(),
        )
    });
    if let Some(module) = glob_module {
        return Some(module);
    }
    let brought = glob_bringing(viewer, module, bindings.globs(), name, &HashSet::new()).is_some()
        || named
            .into_iter()
            .any(|binding| binding.definition().is_some());
    brought.then_some(NameTarget::Item)
}

/// The child module `name` of `module`, or of a module its `pub use` globs
/// republish, where `viewer` sees its declaration and each glob on the way.
fn glob_child_module(
    viewer: Viewer,
    module: CrateModule,
    name: &str,
    module_paths: &ModulePathMap,
    visited: &mut HashSet<(String, String)>,
) -> Option<NameTarget> {
    let viewer = viewer.reaching(module.crate_name);
    if !visited.insert((module.crate_name.to_string(), module.path.to_string())) {
        return None;
    }
    let child = append_to_path(module.path, name);
    let declared = ReExportTarget {
        crate_name: module.crate_name.to_string(),
        module: module.path.to_string(),
        original_name: name.to_string(),
    };
    if module_paths
        .get_or_empty(module.crate_name)
        .contains(&child)
        && module_is_visible(viewer, &declared)
    {
        return Some(NameTarget::Module {
            crate_name: module.crate_name.to_string(),
            path: child,
        });
    }
    viewer
        .bindings_in(module)?
        .globs()
        .filter(|glob| glob.reach == GlobReach::ReExport)
        .find_map(|glob| {
            let source = glob.source_module();
            glob_child_module(viewer.through(module), source, name, module_paths, visited)
        })
}

/// Whether `module` is `ancestor` itself or nested below it. The crate root
/// (`""`) is an ancestor of every module.
fn is_descendant(module: &str, ancestor: &str) -> bool {
    ancestor.is_empty() || module == ancestor || module.starts_with(&format!("{ancestor}::"))
}

/// The deepest module that `module` and `other` are both itself or nested
/// below, as a prefix of `module`: the crate root (`""`) where their first
/// segments differ.
fn common_ancestor<'a>(module: &'a str, other: &str) -> &'a str {
    let (segments, chars) = module
        .split("::")
        .zip(other.split("::"))
        .take_while(|(own, other)| own == other)
        .fold((0_usize, 0), |(segments, chars), (own, _)| {
            (segments + 1, chars + own.len())
        });
    &module[..chars + "::".len() * segments.saturating_sub(1)]
}

/// Invariant context for dependency resolution within a single source file.
pub(crate) struct ResolutionContext<'a> {
    pub(crate) current_crate: &'a str,
    pub(crate) workspace_crates: &'a WorkspacePackages,
    pub(crate) source_file: &'a Path,
    pub(crate) all_module_paths: &'a ModulePathMap,
    pub(crate) crate_exports: &'a PackageExportMap,
    pub(crate) current_module_path: &'a str,
    pub(crate) reexport_map: &'a ReExportMap,
    /// Code-side crate name -> `package_id` for external crates visible to
    /// the current workspace crate. Populated from `crate_name_map[current_crate]`.
    pub(crate) external_crate_names: &'a HashMap<String, String>,
    /// Name -> workspace crate name, for the names the package's manifest
    /// gives its workspace crates (`PackageInfo::code_names`) and from
    /// `extern crate X as Y;` in the root the current file belongs to. Both
    /// put the name in the extern prelude, so it names the crate in every
    /// module of the crate.
    pub(crate) extern_crates: &'a HashMap<String, String>,
}

impl<'a> ResolutionContext<'a> {
    /// The viewer from this file's module, looking through `root`.
    fn viewer(&self, root: TargetRoot) -> Viewer<'a> {
        self.reexport_map.viewer(root, self.current_module_path)
    }

    /// The viewer from this file's module that reads what `dep` points at.
    fn viewer_for(&self, dep: &DependencyRef) -> Viewer<'a> {
        self.viewer(dep.target_root)
    }

    /// The bindings of the module `dep` points at, as this file sees it.
    fn target_bindings(&self, dep: &DependencyRef) -> Option<SeenBindings<'a>> {
        self.viewer_for(dep).bindings_in(CrateModule::of(dep))
    }

    /// [`resolve_reexport_as`] from this file's module, for a reference that
    /// asks `meaning` of `dep`'s item.
    fn resolve_reexport(&self, dep: &mut DependencyRef, meaning: Meaning) -> Resolution {
        resolve_reexport_as(
            dep,
            self.reexport_map,
            self.all_module_paths,
            self.current_module_path,
            meaning,
        )
    }

    /// [`ResolutionContext::resolve_reexport`], with what the chain passed
    /// through noted in `passed_on`. Whether the chain places a definer, so
    /// that `dep` produces an edge.
    fn resolve_noting(
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

/// Promote any context to a test context. Production becomes Unit test;
/// already-test contexts are preserved (idempotent for test contexts).
pub(crate) fn promote_to_test(base: &EdgeContext) -> EdgeContext {
    EdgeContext {
        kind: match base.kind {
            UsageKind::Production => UsageKind::Test(TestKind::Unit),
            already_test @ UsageKind::Test(_) => already_test,
        },
    }
}

/// Where a `use` binds: an inline module body, a block, or the file itself.
pub(crate) type RegionId = usize;

/// The binding regions of one file, as parent links.
///
/// A `use` is an item and binds in its enclosing module or block only. A later
/// reference therefore sees a binding when the binding's region is the
/// reference's own region or one enclosing it.
///
/// Both collectors number the regions of the same file, and a reference from one
/// walk is looked up against bindings from the other. The two numberings have to
/// agree, which is why both take them from `impl_binding_region_visits!` and
/// neither prunes its traversal.
#[derive(Debug, Clone)]
pub(crate) struct BindingRegions {
    parents: Vec<Option<RegionId>>,
}

impl BindingRegions {
    /// The file itself. Encloses every other region and is enclosed by none.
    pub(crate) const ROOT: RegionId = 0;

    fn open(&mut self, parent: RegionId) -> RegionId {
        self.parents.push(Some(parent));
        self.parents.len() - 1
    }

    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.parents.len()
    }

    /// A region and everything enclosing it, innermost first.
    fn enclosing(&self, region: RegionId) -> impl Iterator<Item = RegionId> + '_ {
        std::iter::successors(Some(region), move |&inner| {
            self.parents.get(inner).copied().flatten()
        })
    }
}

impl Default for BindingRegions {
    fn default() -> Self {
        Self {
            parents: vec![None],
        }
    }
}

/// Whether a fn carries `#[test]`. The compiler builds such a fn only into the
/// test harness, with or without `#[cfg(test)]` around it.
fn is_test_fn(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("test"))
}

/// The outer attributes of an item; none for tokens syn could not parse.
pub(crate) fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::ExternCrate(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::ForeignMod(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::TraitAlias(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Union(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// The context inside an item: a test context when the item has
/// `#[cfg(test)]`, the surrounding context otherwise.
fn item_context(item: &syn::Item, outer: &EdgeContext) -> EdgeContext {
    if is_cfg_test(item_attrs(item)) {
        promote_to_test(outer)
    } else {
        outer.clone()
    }
}

/// Shared `visit_item_mod`, `visit_item_fn` and `visit_block` for both collectors.
///
/// `visit_item_mod` and `visit_block` open a binding region. `visit_item_mod`
/// also carries the inline module depth, which counts modules only:
/// `use super::X` in a function body still means the parent module.
/// `visit_item_fn` carries the test context of a fn with `#[test]`; each
/// collector's `visit_item` carries the one of any item with `#[cfg(test)]`.
macro_rules! impl_binding_region_visits {
    () => {
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            let prev_depth = self.inline_depth;
            let prev_region = self.region;
            // Inline modules (with body) add nesting depth; `mod foo;` (external) does not
            if node.content.is_some() {
                self.inline_depth += 1;
                self.region = self.regions.open(prev_region);
            }
            syn::visit::visit_item_mod(self, node);
            self.inline_depth = prev_depth;
            self.region = prev_region;
        }

        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            let prev_context = self.context.clone();
            if is_test_fn(&node.attrs) {
                self.context = promote_to_test(&self.context);
            }
            syn::visit::visit_item_fn(self, node);
            self.context = prev_context;
        }

        fn visit_block(&mut self, node: &'ast syn::Block) {
            let prev_region = self.region;
            self.region = self.regions.open(prev_region);
            syn::visit::visit_block(self, node);
            self.region = prev_region;
        }
    };
}

/// One `use` item with what its position says about it.
#[derive(Clone)]
pub(crate) struct CollectedUse {
    pub(crate) item: syn::ItemUse,
    pub(crate) context: EdgeContext,
    pub(crate) inline_depth: usize,
    pub(crate) region: RegionId,
}

/// The `use` items of one file, with the regions they were written in.
#[derive(Default)]
pub(crate) struct CollectedUses {
    items: Vec<CollectedUse>,
    regions: BindingRegions,
}

impl Deref for CollectedUses {
    type Target = [CollectedUse];

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl CollectedUses {
    /// The `use` items whose context `keep` accepts, in the same regions.
    fn retaining(&self, keep: impl Fn(&EdgeContext) -> bool) -> Self {
        Self {
            items: self
                .items
                .iter()
                .filter(|collected| keep(&collected.context))
                .cloned()
                .collect(),
            regions: self.regions.clone(),
        }
    }
}

/// Collect all `use` items from a parsed file, including those nested inside
/// function bodies, blocks, and other scopes. Uses `syn::visit::Visit` to
/// traverse the full AST regardless of nesting depth.
///
/// Uses inside `#[cfg(test)]` scopes or with `#[cfg(test)]` on the item itself
/// are tagged `Test(Unit)`, all others are `Production`.
pub(crate) fn collect_all_use_items(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> CollectedUses {
    struct UseCollector {
        uses: Vec<CollectedUse>,
        context: EdgeContext,
        inline_depth: usize,
        regions: BindingRegions,
        region: RegionId,
    }
    impl<'ast> Visit<'ast> for UseCollector {
        fn visit_item(&mut self, node: &'ast syn::Item) {
            let prev_context = self.context.clone();
            self.context = item_context(node, &prev_context);
            syn::visit::visit_item(self, node);
            self.context = prev_context;
        }

        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            self.uses.push(CollectedUse {
                item: node.clone(),
                context: self.context.clone(),
                inline_depth: self.inline_depth,
                region: self.region,
            });
        }

        fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
            let prev_context = self.context.clone();
            if is_cfg_test(&node.attrs) {
                self.context = promote_to_test(&self.context);
            }
            syn::visit::visit_impl_item_fn(self, node);
            self.context = prev_context;
        }

        impl_binding_region_visits!();
    }
    let mut collector = UseCollector {
        uses: Vec::new(),
        context: base_context,
        inline_depth: 0,
        regions: BindingRegions::default(),
        region: BindingRegions::ROOT,
    };
    collector.visit_file(syntax);
    CollectedUses {
        items: collector.uses,
        regions: collector.regions,
    }
}

/// Where a path stands. Together with what the path names, this decides the
/// category of the use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathPosition {
    /// A type, a trait bound, or the trait of an `impl`.
    Type,
    /// The callee of a call expression, or the name of a macro.
    Call,
    /// The path of a struct expression or of a struct pattern.
    Construct,
    /// A path read as a value or matched as a pattern, and a path anywhere
    /// else, such as in an attribute.
    Value,
    /// A bare identifier in a pattern. It matches a const, a static or a unit
    /// struct a `use` brings in, and declares a fresh variable otherwise.
    Binding,
}

impl PathPosition {
    /// The meaning this position asks of the last segment of its path.
    fn meaning(self) -> Meaning {
        match self {
            PathPosition::Type | PathPosition::Construct => Meaning::Type,
            PathPosition::Call | PathPosition::Value | PathPosition::Binding => Meaning::Value,
        }
    }
}

/// The meaning a reference asks of a name: one of the three namespaces of the
/// Rust Reference, or any of them. A module stands in the type namespace
/// only, so a call of a name that is a module and a fn calls the fn. A
/// segment that more segments follow is a type. A `use` asks any meaning,
/// since it binds every meaning its target has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Meaning {
    /// Modules, crates, structs, enums, traits, type aliases.
    Type,
    /// Functions, constants, statics, constructors.
    Value,
    /// Macros, and the names in `#[derive(…)]`.
    Macro,
    /// Whatever the name binds.
    Any,
}

impl Meaning {
    /// Whether a module is what this meaning may name.
    fn admits_a_module(self) -> bool {
        matches!(self, Meaning::Type | Meaning::Any)
    }

    /// Whether a binding in `namespace` is what this meaning may name.
    fn admits(self, namespace: Namespace) -> bool {
        match self {
            Meaning::Type => namespace == Namespace::Type,
            Meaning::Value => namespace == Namespace::Value,
            Meaning::Macro => namespace == Namespace::Macro,
            Meaning::Any => true,
        }
    }
}

/// One path reference with what its position says about it.
#[derive(Debug, Clone)]
pub(crate) struct PathRef {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) context: EdgeContext,
    pub(crate) inline_depth: usize,
    pub(crate) region: RegionId,
    pub(crate) position: PathPosition,
    /// What the position asks the last segment to mean: the namespace of its
    /// position, the macro namespace for the name of a macro or a derive, any
    /// for another path read off tokens.
    pub(crate) meaning: Meaning,
    /// Written in macro input or an attribute, where syn does not parse and
    /// the segments are read off the tokens. A binding resolves such a path;
    /// the resolution chain is not asked, so no edge comes from macro input
    /// alone.
    pub(crate) in_macro: bool,
    /// The local variable a bare name in an expression may refer to: the
    /// innermost one of that name in scope where the path is written.
    pub(crate) local: Option<LocalBinding>,
}

impl PathRef {
    /// Resolve this reference through the resolution chain. A path written
    /// in macro input resolves through a binding only.
    fn resolve(&self, ctx: &ResolutionContext) -> Option<DependencyRef> {
        if self.in_macro {
            return None;
        }
        resolve_single_path(
            ctx,
            &self.path,
            self.line,
            &self.context,
            self.inline_depth,
            self.meaning,
        )
    }

    /// Resolve `rest`, what this reference writes after a name bound to
    /// `module`, under that module. Not for a path written in macro input, as
    /// [`PathRef::resolve`].
    fn resolve_under(
        &self,
        ctx: &ResolutionContext,
        module: &BoundModule,
        rest: &str,
    ) -> Option<DependencyRef> {
        if self.in_macro {
            return None;
        }
        module.resolve(
            ctx,
            rest,
            self.line,
            &self.context,
            self.inline_depth,
            self.meaning,
        )
    }

    /// The meaning the path asks of the item it resolved to, with `associated`
    /// the segment it writes after the item: an item the path goes on past is
    /// a type.
    fn meaning_of_item(&self, associated: Option<&str>) -> Meaning {
        if associated.is_some() {
            Meaning::Type
        } else {
            self.meaning
        }
    }
}

/// The pattern that declares a local variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalBinding {
    /// `mut x`, `ref x` or `x @ ..`: a variable for certain.
    Variable,
    /// A bare identifier, the path reference at this index. It declares a
    /// variable unless it matches an item (see [`PathPosition::Binding`]).
    Pattern(usize),
}

/// A local variable while the walk is inside its scope.
#[derive(Debug)]
struct ScopedLocal {
    name: String,
    binding: LocalBinding,
    /// The block or module the variable was declared in. Leaving it ends the
    /// variable's scope.
    region: RegionId,
}

/// The qualified path references of one file, with the regions they stand in.
#[derive(Debug, Default)]
pub(crate) struct CollectedPathRefs {
    refs: Vec<PathRef>,
    /// The regions this walk numbered. A reference is looked up against bindings
    /// numbered by the other walk, so the two numberings have to match. Only the
    /// test that guards that match reads them; resolution goes through the
    /// numbering that came with the bindings.
    #[cfg(test)]
    regions: BindingRegions,
}

impl Deref for CollectedPathRefs {
    type Target = [PathRef];

    fn deref(&self) -> &Self::Target {
        &self.refs
    }
}

impl CollectedPathRefs {
    /// The references whose context `keep` accepts.
    fn retaining(&self, keep: impl Fn(&EdgeContext) -> bool) -> Self {
        Self {
            refs: self
                .refs
                .iter()
                .filter(|path_ref| keep(&path_ref.context))
                .cloned()
                .collect(),
            #[cfg(test)]
            regions: self.regions.clone(),
        }
    }
}

/// The walker behind [`collect_all_path_refs`].
struct PathRefCollector {
    paths: Vec<PathRef>,
    context: EdgeContext,
    inline_depth: usize,
    regions: BindingRegions,
    region: RegionId,
    position: PathPosition,
    /// The local variables in scope, innermost last.
    locals: Vec<ScopedLocal>,
    /// The variables a pattern declares that come into scope only after the
    /// expression the pattern stands before: `let x = <expr>`,
    /// `if let x = <expr>`, `for x in <expr>`.
    declared_later: Option<Vec<ScopedLocal>>,
    /// Inside the name of a macro, which no variable can hide.
    naming_macro: bool,
}
impl PathRefCollector {
    fn in_position(&mut self, position: PathPosition, visit: impl FnOnce(&mut Self)) {
        let prev = self.position;
        self.position = position;
        visit(self);
        self.position = prev;
    }

    /// Visit a scope that ends with the visit: a fn, a closure, a match arm.
    fn in_scope(&mut self, visit: impl FnOnce(&mut Self)) {
        let outer = self.locals.len();
        visit(self);
        self.locals.truncate(outer);
    }

    /// Visit a pattern and return the variables it declares instead of
    /// bringing them into scope.
    fn declarations_of(&mut self, pattern: impl FnOnce(&mut Self)) -> Vec<ScopedLocal> {
        let outer = self.declared_later.replace(Vec::new());
        pattern(self);
        std::mem::replace(&mut self.declared_later, outer).unwrap_or_default()
    }

    fn declare(&mut self, name: String, binding: LocalBinding) {
        let local = ScopedLocal {
            name,
            binding,
            region: self.region,
        };
        match &mut self.declared_later {
            Some(later) => later.push(local),
            None => self.locals.push(local),
        }
    }

    /// The variable a bare name in an expression refers to. Types, struct
    /// paths and macros live in other namespaces than variables.
    fn local_named(&self, path: &str) -> Option<LocalBinding> {
        if self.naming_macro
            || path.contains("::")
            || !matches!(self.position, PathPosition::Value | PathPosition::Call)
        {
            return None;
        }
        self.locals
            .iter()
            .rev()
            .find(|local| {
                local.name == path
                    && self
                        .regions
                        .enclosing(self.region)
                        .any(|r| r == local.region)
            })
            .map(|local| local.binding)
    }

    fn record(&mut self, path: String, line: usize, in_macro: bool) {
        let meaning = if self.naming_macro {
            Meaning::Macro
        } else {
            self.position.meaning()
        };
        self.record_meaning(path, line, in_macro, meaning);
    }

    fn record_meaning(&mut self, path: String, line: usize, in_macro: bool, meaning: Meaning) {
        let local = self.local_named(&path);
        self.paths.push(PathRef {
            path,
            line,
            context: self.context.clone(),
            inline_depth: self.inline_depth,
            region: self.region,
            position: self.position,
            meaning,
            in_macro,
            local,
        });
    }

    /// Read the paths off a token stream syn leaves unparsed: a run of
    /// identifiers joined by `::`, at any nesting of brackets. Every other
    /// token is skipped, so nothing here knows what position a path is in;
    /// `meaning` is what each path is asked to mean, any where nothing says.
    fn record_token_paths(&mut self, tokens: &proc_macro2::TokenStream, meaning: Meaning) {
        use proc_macro2::TokenTree;
        let tokens: Vec<TokenTree> = tokens.clone().into_iter().collect();
        let mut index = 0;
        while index < tokens.len() {
            match &tokens[index] {
                TokenTree::Group(group) => self.record_token_paths(&group.stream(), meaning),
                // A lifetime is a `'` followed by an identifier, not a name.
                TokenTree::Ident(ident) if !is_lifetime_tick(tokens.get(index.wrapping_sub(1))) => {
                    let line = ident.span().start().line;
                    let mut path = vec![ident.to_string()];
                    while is_path_separator(&tokens[index + 1..])
                        && let Some(TokenTree::Ident(next)) = tokens.get(index + 3)
                    {
                        path.push(next.to_string());
                        index += 3;
                    }
                    self.in_position(PathPosition::Value, |s| {
                        s.record_meaning(path.join("::"), line, true, meaning);
                    });
                }
                _ => {}
            }
            index += 1;
        }
    }
}

/// Whether the tokens start with `::`, which arrives as two colons with the
/// first joint to the second.
fn is_path_separator(tokens: &[proc_macro2::TokenTree]) -> bool {
    use proc_macro2::{Spacing, TokenTree};
    matches!(
        tokens,
        [TokenTree::Punct(first), TokenTree::Punct(second), ..]
            if first.as_char() == ':' && first.spacing() == Spacing::Joint && second.as_char() == ':'
    )
}

fn is_lifetime_tick(token: Option<&proc_macro2::TokenTree>) -> bool {
    matches!(token, Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == '\'')
}
impl<'ast> Visit<'ast> for PathRefCollector {
    fn visit_path(&mut self, node: &'ast syn::Path) {
        let path_str: String = node
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        let line = node
            .segments
            .first()
            .map_or(0, |s| s.ident.span().start().line);
        self.record(path_str, line, false);
        syn::visit::visit_path(self, node);
    }

    // Syntax alone cannot tell a const matched from a variable declared; the
    // lookup against the file's bindings and the item's definition can.
    fn visit_pat_ident(&mut self, node: &'ast syn::PatIdent) {
        let binding = if node.by_ref.is_none() && node.mutability.is_none() && node.subpat.is_none()
        {
            self.in_position(PathPosition::Binding, |s| {
                s.record(
                    node.ident.to_string(),
                    node.ident.span().start().line,
                    false,
                );
            });
            LocalBinding::Pattern(self.paths.len() - 1)
        } else {
            LocalBinding::Variable
        };
        self.declare(node.ident.to_string(), binding);
        syn::visit::visit_pat_ident(self, node);
    }

    // An item nested in a fn body sees none of the fn's variables.
    fn visit_item(&mut self, node: &'ast syn::Item) {
        let outer = std::mem::take(&mut self.locals);
        let prev_context = self.context.clone();
        self.context = item_context(node, &prev_context);
        syn::visit::visit_item(self, node);
        self.context = prev_context;
        self.locals = outer;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let prev_context = self.context.clone();
        if is_cfg_test(&node.attrs) {
            self.context = promote_to_test(&self.context);
        }
        self.in_scope(|s| syn::visit::visit_impl_item_fn(s, node));
        self.context = prev_context;
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.in_scope(|s| syn::visit::visit_trait_item_fn(s, node));
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        self.in_scope(|s| syn::visit::visit_expr_closure(s, node));
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        self.in_scope(|s| syn::visit::visit_arm(s, node));
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        let declared = self.declarations_of(|s| s.visit_pat(&node.pat));
        if let Some(init) = &node.init {
            self.visit_local_init(init);
        }
        self.locals.extend(declared);
    }

    // The variables of an `if let` are in scope in the condition after it and
    // in the `then` block, not in the `else` branch.
    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        self.in_scope(|s| {
            s.visit_expr(&node.cond);
            s.visit_block(&node.then_branch);
        });
        if let Some((_, else_branch)) = &node.else_branch {
            self.visit_expr(else_branch);
        }
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.in_scope(|s| syn::visit::visit_expr_while(s, node));
    }

    fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        let declared = self.declarations_of(|s| s.visit_pat(&node.pat));
        self.visit_expr(&node.expr);
        self.locals.extend(declared);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        if let Some(label) = &node.label {
            self.visit_label(label);
        }
        let declared = self.declarations_of(|s| s.visit_pat(&node.pat));
        self.visit_expr(&node.expr);
        self.in_scope(|s| {
            s.locals.extend(declared);
            s.visit_block(&node.body);
        });
    }

    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        self.in_position(PathPosition::Type, |s| syn::visit::visit_type_path(s, node));
    }

    fn visit_trait_bound(&mut self, node: &'ast syn::TraitBound) {
        self.in_position(PathPosition::Type, |s| {
            syn::visit::visit_trait_bound(s, node);
        });
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.in_position(PathPosition::Type, |s| syn::visit::visit_item_impl(s, node));
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        self.in_position(PathPosition::Value, |s| {
            syn::visit::visit_expr_path(s, node);
        });
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(func) = &*node.func {
            self.in_position(PathPosition::Call, |s| syn::visit::visit_expr_path(s, func));
            for arg in &node.args {
                self.visit_expr(arg);
            }
        } else {
            syn::visit::visit_expr_call(self, node);
        }
    }

    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        self.in_position(PathPosition::Construct, |s| {
            syn::visit::visit_expr_struct(s, node);
        });
    }

    fn visit_pat_struct(&mut self, node: &'ast syn::PatStruct) {
        self.in_position(PathPosition::Construct, |s| {
            syn::visit::visit_pat_struct(s, node);
        });
    }

    fn visit_pat_tuple_struct(&mut self, node: &'ast syn::PatTupleStruct) {
        self.in_position(PathPosition::Construct, |s| {
            syn::visit::visit_pat_tuple_struct(s, node);
        });
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.naming_macro = true;
        self.in_position(PathPosition::Call, |s| syn::visit::visit_macro(s, node));
        self.naming_macro = false;
        self.record_token_paths(&node.tokens, Meaning::Any);
    }

    // `#[derive(Serialize)]` and the like name what they use in tokens too.
    // A name in `#[derive(…)]` is a derive macro.
    fn visit_meta_list(&mut self, node: &'ast syn::MetaList) {
        syn::visit::visit_meta_list(self, node);
        let meaning = if node.path.is_ident("derive") {
            Meaning::Macro
        } else {
            Meaning::Any
        };
        self.record_token_paths(&node.tokens, meaning);
    }

    // `pub(in path)` names the scope that may see the item; nothing is used
    // from that module, so its path is no dependency.
    fn visit_vis_restricted(&mut self, _node: &'ast syn::VisRestricted) {}

    impl_binding_region_visits!();
}

/// Collect all path references from a parsed file, each with its position.
/// Uses `syn::visit::Visit` to traverse expressions, types, patterns, and trait bounds.
/// References inside `#[cfg(test)]` scopes are tagged `Test(Unit)`, all others
/// `Production`.
///
/// A single-segment path is kept too: it may name what a `use` in the file
/// binds. The position is the innermost syntactic context around the path;
/// the collector sets it on entering that context, so a path nested in another
/// path's generic arguments gets its own.
pub(crate) fn collect_all_path_refs(
    syntax: &syn::File,
    base_context: EdgeContext,
) -> CollectedPathRefs {
    let mut collector = PathRefCollector {
        paths: Vec::new(),
        context: base_context,
        inline_depth: 0,
        regions: BindingRegions::default(),
        region: BindingRegions::ROOT,
        position: PathPosition::Value,
        locals: Vec::new(),
        declared_later: None,
        naming_macro: false,
    };
    collector.visit_file(syntax);
    CollectedPathRefs {
        refs: collector.paths,
        #[cfg(test)]
        regions: collector.regions,
    }
}

/// Join a prefix and segment with `::`, handling empty prefix.
fn append_to_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
    }
}

/// Whether a use-tree leaf is the `self` that names its parent module.
/// A `self` without a prefix (`use self;`) does not name a module and is left alone.
fn is_self_segment(ident: &syn::Ident, prefix: &str) -> bool {
    ident == "self" && !prefix.is_empty()
}

/// Recursively resolve a `syn::UseTree` into fully-qualified path strings.
///
/// Example: `use cli::{Args, Cargo, run}` → `["cli::Args", "cli::Cargo", "cli::run"]`
///
/// When `use_alias` is true, renames return the alias (`as X` → `X`).
/// When false, renames return the original name (source dependency tracking).
///
/// A trailing `self` (`use foo::{self, ..}`) names the module `foo` itself, so it
/// resolves to the prefix rather than to an item called `self`. Under `use_alias`
/// that keeps the local binding name as the last segment (`foo`), which is what an
/// unrenamed `self` binds.
pub(crate) fn resolve_use_tree(tree: &UseTree, prefix: &str, use_alias: bool) -> Vec<String> {
    match tree {
        UseTree::Path(p) => resolve_use_tree(
            &p.tree,
            &append_to_path(prefix, &p.ident.to_string()),
            use_alias,
        ),
        UseTree::Name(n) if is_self_segment(&n.ident, prefix) => vec![prefix.to_string()],
        UseTree::Name(n) => vec![append_to_path(prefix, &n.ident.to_string())],
        // `use foo::{self as bar}`: the alias side binds `bar`, the source side is `foo`.
        UseTree::Rename(r) if !use_alias && is_self_segment(&r.ident, prefix) => {
            vec![prefix.to_string()]
        }
        UseTree::Rename(r) => {
            let name = if use_alias { &r.rename } else { &r.ident };
            vec![append_to_path(prefix, &name.to_string())]
        }
        UseTree::Glob(_) => vec![append_to_path(prefix, "*")],
        UseTree::Group(g) => g
            .items
            .iter()
            .flat_map(|item| resolve_use_tree(item, prefix, use_alias))
            .collect(),
    }
}

/// Find the longest module path prefix from `parts` that exists in `module_paths`.
///
/// Tries from longest to shortest: `["analyze", "use_parser", "normalize"]`
/// checks `"analyze::use_parser"`, then `"analyze"`.
/// Returns `(matched_path, segment_count)`.
/// Fallback: first segment with count 1.
fn find_longest_module_prefix(parts: &[&str], module_paths: &HashSet<String>) -> (String, usize) {
    for end in (1..=parts.len()).rev() {
        let candidate: String = parts[..end].join("::");
        if module_paths.contains(&candidate) {
            return (candidate, end);
        }
    }
    // Fallback: first segment
    (parts[0].to_string(), 1)
}

/// The module and item `parts` name in the own crate `crate_name`. A name
/// after the longest module prefix that stands for another module of the
/// crate there, as [`name_in_module`] finds it from the file's module, carries
/// the rest of the path into that module: beside
/// `use crate::store::{self as bound};` in `holder`, `holder::bound::Set`
/// names `Set` in `store`.
fn module_and_item(
    ctx: &ResolutionContext,
    crate_name: &str,
    parts: &[&str],
    module_paths: &HashSet<String>,
) -> (String, Option<String>) {
    let mut parts: Vec<String> = parts.iter().map(ToString::to_string).collect();
    let mut visited = HashSet::new();
    loop {
        let segments: Vec<&str> = parts.iter().map(String::as_str).collect();
        let (module, prefix_len) = find_longest_module_prefix(&segments, module_paths);
        let item = extract_item_from_parts(&segments, prefix_len);
        let bound = item
            .as_deref()
            .filter(|_| segments.len() > prefix_len + 1 && module_paths.contains(&module))
            .filter(|name| visited.insert((module.clone(), (*name).to_string())))
            .and_then(|name| {
                let holder = CrateModule {
                    crate_name,
                    path: &module,
                };
                let viewer = ctx.viewer(TargetRoot::AsSeen);
                match name_in_module(viewer, holder, name, ctx.all_module_paths, true)? {
                    NameTarget::Module {
                        crate_name: bound_crate,
                        path,
                    } if bound_crate == crate_name && !path.is_empty() => Some(path),
                    NameTarget::Module { .. } | NameTarget::Item => None,
                }
            });
        let Some(bound) = bound else {
            return (module, item);
        };
        parts = bound
            .split("::")
            .map(ToString::to_string)
            .chain(parts[prefix_len + 1..].iter().cloned())
            .collect();
    }
}

/// The path that starts at `crate_name`, runs to its module `module` (`""`
/// for the root) and goes on with `rest`.
fn path_from_crate(crate_name: &str, module: &str, rest: &[&str]) -> String {
    std::iter::once(crate_name)
        .chain((!module.is_empty()).then_some(module))
        .chain(rest.iter().copied())
        .collect::<Vec<_>>()
        .join("::")
}

/// Extract an item from path parts at given index, handling trailing `{` and empty strings.
fn extract_item_from_parts(parts: &[&str], index: usize) -> Option<String> {
    let part = parts
        .get(index)?
        .trim_end_matches('{')
        .trim_end_matches(';')
        .trim();
    if part.is_empty() || part.starts_with('{') {
        None
    } else {
        Some(part.to_string())
    }
}

/// Parse crate-local imports: `use crate::module[::item]`
fn parse_crate_local_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let after_crate = path.strip_prefix("crate::")?;
    let parts: Vec<&str> = after_crate.split("::").collect();

    let first = parts.first()?.trim_end_matches('{').trim();
    if first.is_empty() {
        return None;
    }

    let crate_name = normalize_crate_name(ctx.current_crate);
    let module_paths = ctx.all_module_paths.get_or_empty(&crate_name);
    let rest = &parts[1..];
    let root = CrateModule {
        crate_name: &crate_name,
        path: "",
    };
    let named = name_in_module(
        ctx.viewer(TargetRoot::AsSeen),
        root,
        first,
        ctx.all_module_paths,
        !rest.is_empty(),
    );
    let (target_module, target_item) = match named {
        Some(NameTarget::Module {
            crate_name: other,
            path,
        }) if other != crate_name => {
            if rest.is_empty() {
                (String::new(), Some(first.to_string()))
            } else {
                let in_other = path_from_crate(&other, &path, rest);
                return parse_workspace_import(ctx, &in_other, line_num, context);
            }
        }
        Some(NameTarget::Module { path, .. }) => {
            let in_crate: Vec<&str> = path
                .split("::")
                .filter(|segment| !segment.is_empty())
                .chain(rest.iter().copied())
                .collect();
            if in_crate.is_empty() {
                (String::new(), Some(first.to_string()))
            } else {
                module_and_item(ctx, &crate_name, &in_crate, module_paths)
            }
        }
        Some(NameTarget::Item) => (String::new(), Some(first.to_string())),
        None => module_and_item(ctx, &crate_name, &parts, module_paths),
    };

    Some(DependencyRef {
        target_package: crate_name,
        target_module,
        target_item,
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
        via_reexport: false,
        uses: Vec::new(),
        target_root: TargetRoot::AsSeen,
    })
}

/// Parse bare module imports: `use cli::Args` where `cli` is a module in scope
/// of the file.
///
/// `ctx.current_module_path` is the relative module path of the file containing
/// this import (e.g. `"render"` for `render/mod.rs`, `""` for crate root). A
/// bare first segment names a child of that module: `use css::X` in
/// `render/mod.rs` is `render::css`, `use cli::X` in the root is `cli`. A
/// top-level module is a child of the root only; from any other file its bare
/// name is not in scope, and what stands there is an extern crate or a name a
/// `use` bound.
fn parse_bare_module_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let first = parts.first()?.trim_end_matches('{').trim();
    if first.is_empty() {
        return None;
    }

    let module_paths = ctx
        .all_module_paths
        .get_or_empty(&normalize_crate_name(ctx.current_crate));

    let effective_parts: Vec<&str> = if ctx.current_module_path.is_empty() {
        if !module_paths.contains(first) {
            return None;
        }
        parts
    } else if module_paths.contains(&format!("{}::{first}", ctx.current_module_path)) {
        ctx.current_module_path
            .split("::")
            .chain(parts.iter().copied())
            .collect()
    } else {
        return None;
    };

    let crate_name = normalize_crate_name(ctx.current_crate);
    let (target_module, target_item) =
        module_and_item(ctx, &crate_name, &effective_parts, module_paths);

    Some(DependencyRef {
        target_package: crate_name,
        target_module,
        target_item,
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
        via_reexport: false,
        uses: Vec::new(),
        target_root: TargetRoot::AsSeen,
    })
}

/// Parse workspace crate imports: `use other_crate::module[::item]`
fn parse_workspace_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let crate_name = parts.first()?.trim();

    if !ctx.workspace_crates.contains(crate_name) || parts.len() < 2 {
        return None;
    }

    let module_segment = parts[1].trim_end_matches('{').trim_end_matches(';').trim();
    if module_segment.is_empty() {
        return None;
    }

    let target_crate_name = normalize_crate_name(crate_name);
    let target_root = root_named_by(ctx, &target_crate_name);
    let module_paths = ctx.all_module_paths.get_or_empty(&target_crate_name);
    let rest = &parts[2..];
    let root = CrateModule {
        crate_name: &target_crate_name,
        path: "",
    };
    let named = name_in_module(
        ctx.viewer(target_root),
        root,
        module_segment,
        ctx.all_module_paths,
        !rest.is_empty(),
    );
    let in_crate: Vec<&str> = match &named {
        Some(NameTarget::Module {
            crate_name: other,
            path,
        }) if *other != target_crate_name && !rest.is_empty() => {
            let in_other = path_from_crate(other, path, rest);
            return parse_workspace_import(ctx, &in_other, line_num, context);
        }
        Some(NameTarget::Module {
            crate_name: other,
            path,
        }) if *other == target_crate_name && !path.is_empty() => {
            path.split("::").chain(rest.iter().copied()).collect()
        }
        _ => parts[1..].to_vec(),
    };
    let (target_module, prefix_len) = find_longest_module_prefix(&in_crate, module_paths);

    // Entry-point detection: if the resolved target_module is not a known module
    // and the first segment after the crate name is a known export, treat it as
    // an entry-point dependency (target_module = ""). `crate_exports` reads only
    // the root file, so a name a `use` or a glob binds in the root is looked up
    // in the map.
    let bound_in_root = match &named {
        Some(NameTarget::Item) => true,
        Some(NameTarget::Module { crate_name, .. }) => *crate_name != target_crate_name,
        None => false,
    };
    let is_entry_point = !module_paths.contains(&target_module)
        && (bound_in_root
            || ctx
                .crate_exports
                .get(&target_crate_name)
                .is_some_and(|e| e.contains(module_segment)));

    let (target_module, target_item) = if is_entry_point {
        (String::new(), Some(module_segment.to_string()))
    } else {
        (
            target_module,
            extract_item_from_parts(&in_crate, prefix_len),
        )
    };

    Some(DependencyRef {
        target_package: crate_name.to_string(),
        target_module,
        target_item,
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
        via_reexport: false,
        uses: Vec::new(),
        target_root,
    })
}

/// Resolve `super::` and `self::` relative paths to absolute crate-local paths.
///
/// Returns `None` when the path is not relative, when `super::`/`self::` is absorbed
/// by inline module depth, or when too many `super::` would go above crate root.
fn resolve_relative_path(
    path: &str,
    current_module_path: &str,
    inline_depth: usize,
) -> Option<String> {
    let segments: Vec<&str> = path.split("::").collect();
    let super_count = segments.iter().take_while(|&&s| s == "super").count();

    if super_count > inline_depth {
        let levels_up = super_count - inline_depth;
        return join_module_segments(current_module_path, levels_up, &segments[super_count..]);
    }

    if segments.first() == Some(&"self") && inline_depth == 0 {
        return join_module_segments(current_module_path, 0, &segments[1..]);
    }

    None
}

/// Strip `levels_up` trailing segments from `base_path`, append `suffix`, join with `::`.
/// Returns `None` if `levels_up` exceeds the number of segments in `base_path`.
fn join_module_segments(base_path: &str, levels_up: usize, suffix: &[&str]) -> Option<String> {
    let mut base: Vec<&str> = base_path.split("::").filter(|s| !s.is_empty()).collect();
    if levels_up > base.len() {
        return None;
    }
    base.truncate(base.len() - levels_up);
    base.extend_from_slice(suffix);
    Some(base.join("::"))
}

/// Resolve a single use path through the resolution chain: crate-local → bare module → workspace.
/// Handles glob paths (`crate::module::*`) by stripping the glob and setting `target_item` = "*".
/// `meaning` is what the reference asks of the path's last segment.
pub(crate) fn resolve_single_path(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
    inline_depth: usize,
    meaning: Meaning,
) -> Option<DependencyRef> {
    let mut dep = resolve_path_in_chain(ctx, path, line_num, context, inline_depth)?;
    take_item_the_parent_reexports(ctx, &mut dep, meaning);
    Some(dep)
}

/// Point a dependency on a module at the item of the same name in the
/// module's parent, where the reference does not mean the module. Beside
/// `mod helper; pub use helper::helper;` the path `parent::helper` names the
/// module and the fn. A call or a value means the fn, where the parent holds
/// one; an item the parent does not show keeps the module. Otherwise only the
/// fn is visible outside `parent`: a file that sees the module, as its
/// declaration says from the file's crate, root and module, keeps its
/// dependency on the module. A re-export of a module under that name keeps the
/// dependency on the module, and so does a public module, which is visible
/// wherever the item is.
fn take_item_the_parent_reexports(
    ctx: &ResolutionContext,
    dep: &mut DependencyRef,
    meaning: Meaning,
) {
    if dep.target_item.is_some() || dep.target_module.is_empty() {
        return;
    }
    let (parent, name) = dep
        .target_module
        .rsplit_once("::")
        .unwrap_or(("", dep.target_module.as_str()));
    let viewer = ctx.viewer_for(dep);
    let parent_module = CrateModule {
        crate_name: &dep.target_package,
        path: parent,
    };
    if !meaning.admits_a_module()
        && holds_an_item_named(viewer, ctx.all_module_paths, parent_module, name)
    {
        let (parent, name) = (parent.to_string(), name.to_string());
        dep.target_module = parent;
        dep.target_item = Some(name);
        return;
    }
    let Some(target) = viewer
        .bindings_in(parent_module)
        .filter(|bindings| !sees_the_module(*bindings, name))
        .and_then(|bindings| bindings.named(name).find_map(NameBinding::reexported))
    else {
        return;
    };
    if !binds_a_module(target, ctx.all_module_paths) {
        let (parent, name) = (parent.to_string(), name.to_string());
        dep.target_module = parent;
        dep.target_item = Some(name);
    }
}

/// The resolution chain of [`resolve_single_path`].
fn resolve_path_in_chain(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
    inline_depth: usize,
) -> Option<DependencyRef> {
    // Resolve super::/self:: to absolute crate-local path, then route to crate:: handler
    if let Some(resolved) = resolve_relative_path(path, ctx.current_module_path, inline_depth) {
        let as_crate_path = format!("crate::{resolved}");
        return parse_crate_local_import(ctx, &as_crate_path, line_num, context);
    }

    // Handle glob: `crate::module::*` → resolve base, set target_item = "*".
    // `crate::*` has no module to resolve and is the root's glob.
    if let Some(base) = path.strip_suffix("::*") {
        let mut dep = if base == "crate" {
            DependencyRef {
                target_package: normalize_crate_name(ctx.current_crate),
                target_module: String::new(),
                target_item: None,
                source_file: ctx.source_file.to_path_buf(),
                line: line_num,
                context: context.clone(),
                via_reexport: false,
                uses: Vec::new(),
                target_root: TargetRoot::AsSeen,
            }
        } else {
            resolve_path_in_chain(ctx, base, line_num, context, inline_depth)?
        };
        // The base resolved as a module — push "*" as the item
        dep.target_item = Some("*".to_string());
        return Some(dep);
    }

    // As many `super::` as inline modules leave them all and land in the file's
    // module, where the rest is a bare path: `super::child::X` names a child.
    if inline_depth > 0 {
        let supers = vec!["super"; inline_depth].join("::");
        if let Some(rest) = path
            .strip_prefix(supers.as_str())
            .and_then(|tail| tail.strip_prefix("::"))
        {
            return parse_bare_module_import(ctx, rest, line_num, context);
        }
    }

    // A name the crate writes for a workspace crate other than its key names
    // that crate, even where the name is another workspace crate's key.
    let dealiased = dealias_extern_crate(ctx, path);
    let in_workspace = dealiased.as_deref().unwrap_or(path);
    parse_crate_local_import(ctx, path, line_num, context)
        .or_else(|| parse_bare_module_import(ctx, path, line_num, context))
        .or_else(|| parse_workspace_import(ctx, in_workspace, line_num, context))
        .or_else(|| workspace_crate_root(ctx, in_workspace, line_num, context))
        .or_else(|| parse_external_crate_import(ctx, path, line_num, context))
}

/// The root of the workspace crate `path` names bare, as `use other_crate::{Foo}`
/// leaves `other_crate`.
fn workspace_crate_root(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    if path.contains("::") || !ctx.workspace_crates.contains(path) {
        return None;
    }
    Some(DependencyRef {
        target_package: path.to_string(),
        target_module: String::new(),
        target_item: None,
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
        via_reexport: false,
        uses: Vec::new(),
        target_root: root_named_by(ctx, &normalize_crate_name(path)),
    })
}

/// The root a path through the name of the workspace crate `crate_name`
/// resolves against: the own package by name is its lib, also from a file of
/// the bin tree.
fn root_named_by(ctx: &ResolutionContext, crate_name: &str) -> TargetRoot {
    if crate_name == normalize_crate_name(ctx.current_crate) {
        TargetRoot::Lib
    } else {
        TargetRoot::AsSeen
    }
}

/// Resolve `path`, written from the root of `package`, a workspace crate other
/// than this one or an external crate. No name of this crate is asked: the
/// package is known. A glob keeps its `*` as the item.
fn resolve_in_package(
    ctx: &ResolutionContext,
    package: &str,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    if let Some(base) = path
        .strip_suffix("::*")
        .or_else(|| (path == "*").then_some(""))
    {
        let mut dep = resolve_in_package(ctx, package, base, line_num, context)?;
        dep.target_item = Some("*".to_string());
        return Some(dep);
    }
    let full = if path.is_empty() {
        package.to_string()
    } else {
        format!("{package}::{path}")
    };
    parse_workspace_import(ctx, &full, line_num, context)
        .or_else(|| workspace_crate_root(ctx, &full, line_num, context))
        .or_else(|| parse_external_crate_import(ctx, &full, line_num, context))
}

/// `path` with a leading name from `extern_crates` replaced by the crate it
/// names, also when the name is all of `path`.
fn dealias_extern_crate(ctx: &ResolutionContext, path: &str) -> Option<String> {
    let (first, rest) = path
        .split_once("::")
        .map_or((path, None), |(first, rest)| (first, Some(rest)));
    let crate_name = ctx.extern_crates.get(first)?;
    Some(rest.map_or_else(
        || crate_name.clone(),
        |rest| format!("{crate_name}::{rest}"),
    ))
}

/// Parse external crate imports: `use serde::Deserialize` where `serde` is a known external crate.
/// Fallback at the end of the resolution chain — only matches if no workspace resolution succeeded.
fn parse_external_crate_import(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let first = parts.first()?.trim();
    if first.is_empty() {
        return None;
    }

    // Check if the first path segment is a known external crate name
    if !ctx.external_crate_names.contains_key(first) {
        return None;
    }

    let target_item = if parts.len() > 1 {
        Some(parts[1..].join("::"))
    } else {
        None
    };

    Some(DependencyRef {
        target_package: first.to_string(),
        target_module: String::new(),
        target_item,
        source_file: ctx.source_file.to_path_buf(),
        line: line_num,
        context: context.clone(),
        via_reexport: false,
        uses: Vec::new(),
        target_root: TargetRoot::AsSeen,
    })
}

/// Parse syn-based use items, extracting workspace-relevant dependencies.
///
/// Returns `DependencyRefs` for:
/// - Crate-local imports (`use crate::module`)
/// - Workspace crate imports (`use other_crate::module` where `other_crate` is in workspace)
///
/// Deduplicates by `full_target()` to keep distinct symbols but avoid duplicates.
pub(crate) fn parse_workspace_dependencies(
    use_items: &CollectedUses,
    ctx: &ResolutionContext,
    bindings: &FileBindings,
    passed_on: &mut PassedOn,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashMap<(String, UsageKind), usize> = HashMap::new();

    for collected in use_items.iter() {
        let item = &collected.item;
        let paths = resolve_use_tree(&item.tree, "", false);
        // A `use` that republishes a name under a visibility reachable from
        // outside the module is a re-export. Path references (non-use) are never
        // re-exports.
        let via_reexport = is_reexport_visibility(&item.vis);

        // A `use` binds every meaning of its target.
        for path in paths {
            if let Some(mut dep) = bindings.resolve_use_path(ctx, &path, collected) {
                // A private glob whose target the map knows is resolved into
                // the names the file uses, by `parse_path_ref_dependencies`.
                // A `pub use` glob republishes every name and uses none, so it
                // is the whole payload.
                if is_glob(&dep) && glob_payload_known(&dep, ctx) && !via_reexport {
                    continue;
                }
                dep.via_reexport = via_reexport;
                for mut dep in expand_glob(dep, ctx) {
                    if ctx.resolve_noting(&mut dep, Meaning::Any, passed_on) {
                        DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
                    }
                }
            }
        }
    }

    deps
}

/// The names `use <module>::*` in the file of `ctx` brings into scope: the
/// module's own definitions plus what any of its `use` items binds under a
/// name, as far as the file's module sees them.
///
/// `None` when the module is absent from the map or exports nothing under a name
/// the file's module sees. Sorted, because the map iterates in arbitrary order and
/// the names reach the rendered output.
fn glob_payload(dep: &DependencyRef, ctx: &ResolutionContext) -> Option<Vec<String>> {
    let viewer = ctx.viewer_for(dep);
    let bindings = viewer.bindings_in(CrateModule::of(dep))?;
    let mut names: Vec<String> = bindings
        .all()
        .filter(|(_, binding)| {
            matches!(
                binding.kind,
                BindingKind::Definition(_) | BindingKind::ReExport(_) | BindingKind::PrivateUse(_)
            )
        })
        .map(|(name, _)| name.to_string())
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort_unstable();
    names.dedup();
    Some(names)
}

/// Split a re-export glob into one dependency per name it republishes, so an
/// edge weighs what crosses it rather than the one line that spells it.
///
/// Non-globs pass through. So does a glob whose payload is unknown: it keeps the
/// `*`, which scores as a single unnamed symbol — an understatement, but a
/// smaller one than dropping the edge to zero.
fn expand_glob(dep: DependencyRef, ctx: &ResolutionContext) -> Vec<DependencyRef> {
    if !is_glob(&dep) {
        return vec![dep];
    }
    let Some(names) = glob_payload(&dep, ctx) else {
        return vec![dep];
    };
    names
        .into_iter()
        .map(|name| DependencyRef {
            target_item: Some(name),
            ..dep.clone()
        })
        .collect()
}

/// The `*` marker on a glob's dependency. It must survive upstream of here:
/// `collect_use_reexports` reads it out of its own `resolve_use_tree` pass to
/// hold the module's globs, which is what `resolve_reexport` resolves glob
/// chains against.
fn is_glob(dep: &DependencyRef) -> bool {
    dep.target_item.as_deref() == Some("*")
}

/// Whether the map holds the module a glob imports from, and so can say which
/// names the glob brings in.
fn glob_payload_known(dep: &DependencyRef, ctx: &ResolutionContext) -> bool {
    ctx.target_bindings(dep).is_some()
}

/// What every name in one file means, per region: module aliases, names bound
/// elsewhere, names bound to items, glob imports, and the file's own item
/// names.
///
/// `modules` holds a binding name → the module it names, by package and path,
/// so a binding stays valid wherever in the file it is used.
///
/// `elsewhere` holds the names bound to something this crate does not contain.
/// The parser need not know what that something is. An import from a crate it has
/// no metadata for still binds the name, and the module beside the file is then not
/// what a bare path starting with that name means.
///
/// `items` holds the names bound to an item, with the dependency the `use`
/// resolved to. A later path starting with such a name is an occurrence of that
/// item, and what follows the name is an associated item of it.
///
/// `globs` holds the glob imports whose payload the map knows, as the
/// dependency on the module with the `*` still in place. A name nothing else
/// binds and the file does not define is the glob's where the module exports
/// it.
///
/// `glob_modules` holds, by name and like `modules`, the
/// child modules of a glob's module and the names that module binds to a
/// module by `pub use` or, where the file sits below it in the same crate, by
/// a private `use`. A glob brings them in like any other name, so a path may
/// start with one.
#[derive(Debug, Default)]
pub(crate) struct FileBindings {
    regions: BindingRegions,
    modules: HashMap<RegionId, HashMap<String, BoundModule>>,
    elsewhere: HashMap<RegionId, HashSet<String>>,
    items: HashMap<RegionId, HashMap<String, DependencyRef>>,
    globs: HashMap<RegionId, Vec<DependencyRef>>,
    glob_modules: HashMap<RegionId, HashMap<String, BoundModule>>,
    /// The names of the items the file defines, at any depth. A glob never
    /// brings in a name the file defines itself.
    local_definitions: HashSet<String>,
}

/// What one path reference stands for, given the bindings around it.
enum PathBinding<'a> {
    /// A path whose first segment is bound to a module: the module and what
    /// the path writes after the segment.
    Module(&'a BoundModule, &'a str),
    /// An occurrence of an item a `use` binds, plus the associated item the
    /// path goes on to name: `Config::new` on the binding `Config` leaves
    /// `new`.
    Item(&'a DependencyRef, Option<&'a str>),
    /// A path whose first segment is bound to something this crate does not
    /// contain.
    Elsewhere,
    /// A bare name nothing binds, split into the name and what follows it.
    /// It may be a glob's.
    Unbound(&'a str, Option<&'a str>),
}

impl FileBindings {
    /// Build the bindings of one file from its `use` items and the items it
    /// defines.
    ///
    /// `use crate::device::{queue, Device}` binds `queue` to the module
    /// `device::queue`; a later `queue::TempResource` is only resolvable against
    /// that binding. What each binding means for later references is decided in
    /// [`classify_binding`].
    pub(crate) fn of(
        syntax: &syn::File,
        use_items: &CollectedUses,
        ctx: &ResolutionContext,
    ) -> FileBindings {
        let mut bindings = FileBindings {
            regions: use_items.regions.clone(),
            ..FileBindings::default()
        };

        // A `use` path may start with a module another `use` or a glob binds,
        // written before or after it. Those bindings come first; a chain
        // through two such bindings is followed one step only.
        for (collected, binding, original_path) in use_bindings(use_items) {
            let dep = resolve_single_path(
                ctx,
                &original_path,
                collected.item.use_token.span.start().line,
                &collected.context,
                collected.inline_depth,
                Meaning::Any,
            );
            if binding == "*" {
                if let Some(dep) = dep {
                    bindings.bind_glob_modules(collected.region, ctx, &dep);
                }
                continue;
            }
            let chained = through_the_chain(ctx, dep.as_ref());
            if let Binding::Module(module) = classify_binding(ctx, chained.as_ref(), &binding) {
                bindings.bind_module(collected.region, &binding, module);
            }
        }

        for (collected, binding, original_path) in use_bindings(use_items) {
            let binding = binding.as_str();
            let dep = bindings.resolve_use_path(ctx, &original_path, collected);
            // A glob binds every name its module exports and none of them is
            // written here; the names are found where the file uses them.
            if binding == "*" {
                if let Some(mut dep) = dep
                    && glob_payload_known(&dep, ctx)
                {
                    dep.via_reexport = is_reexport_visibility(&collected.item.vis);
                    bindings.bind_glob(collected.region, dep);
                }
                continue;
            }
            // The arc of the `use` ends where the chain does, so the import is
            // noted under that target.
            let chained = through_the_chain(ctx, dep.as_ref());
            match classify_binding(ctx, chained.as_ref(), binding) {
                Binding::Module(mut module) => {
                    module.import = chained.as_ref().map(DependencyRef::full_target);
                    bindings.bind_module(collected.region, binding, module);
                }
                Binding::Elsewhere => bindings.bind_elsewhere(collected.region, binding),
                Binding::OwnItem => {}
            }
            // A name bound to an item of any crate the parser placed carries the
            // item's occurrences. A name bound elsewhere may also be one: the
            // parser has no module for it, but it knows the crate.
            if let Some(mut dep) = dep
                && dep.target_item.is_some()
            {
                dep.via_reexport = is_reexport_visibility(&collected.item.vis);
                bindings.bind_item(collected.region, binding, dep);
            }
        }

        bindings.define_locally(syntax);
        bindings
    }

    /// Look up what a reference standing in `region` stands for.
    ///
    /// `queue::TempResource` under `queue → crate::device::queue` is the path
    /// `crate::device::queue::TempResource`. `Config::new` under an item binding
    /// `Config` is an occurrence of that item. A first segment bound elsewhere
    /// rules out the module beside the file, and nothing here says what to put
    /// in its place. A path nothing binds is `Unbound`: qualified, it may still
    /// resolve on its own; bare, it resolves through a glob or not at all.
    ///
    /// Only bindings written in `region` or around it are asked. A `use` in the
    /// body of one function says nothing about the function beside it, so the
    /// innermost region that binds the name wins and a sibling region is never
    /// consulted.
    fn binding_of<'p>(&'p self, path: &'p str, region: RegionId) -> PathBinding<'p> {
        let (first, rest) = match path.split_once("::") {
            Some((first, rest)) => (first, Some(rest)),
            None => (path, None),
        };
        let last = rest.and_then(|r| r.rsplit("::").next());
        for enclosing in self.regions.enclosing(region) {
            if let Some(module) = self.modules.get(&enclosing).and_then(|m| m.get(first))
                && let Some(rest) = rest
            {
                return PathBinding::Module(module, rest);
            }
            if let Some(dep) = self.items.get(&enclosing).and_then(|m| m.get(first)) {
                return PathBinding::Item(dep, last);
            }
            if self
                .elsewhere
                .get(&enclosing)
                .is_some_and(|names| names.contains(first))
            {
                return PathBinding::Elsewhere;
            }
            if let Some(module) = self.glob_modules.get(&enclosing).and_then(|m| m.get(first))
                && let Some(rest) = rest
                && !self.local_definitions.contains(first)
            {
                return PathBinding::Module(module, rest);
            }
        }
        PathBinding::Unbound(first, last)
    }

    /// Resolve the path `collected` imports, under the module its first
    /// segment is bound to where it is bound to one.
    fn resolve_use_path(
        &self,
        ctx: &ResolutionContext,
        path: &str,
        collected: &CollectedUse,
    ) -> Option<DependencyRef> {
        let line = collected.item.use_token.span.start().line;
        let (context, inline_depth) = (&collected.context, collected.inline_depth);
        match self.binding_of(path, collected.region) {
            PathBinding::Module(module, rest) => {
                module.resolve(ctx, rest, line, context, inline_depth, Meaning::Any)
            }
            _ => resolve_single_path(ctx, path, line, context, inline_depth, Meaning::Any),
        }
    }

    /// List the glob imports a name standing in `region` may come from, one
    /// slice per scope, innermost first. Empty when the file defines the name
    /// itself.
    fn glob_scopes_for(
        &self,
        name: &str,
        region: RegionId,
    ) -> impl Iterator<Item = &[DependencyRef]> {
        let defined_here = self.local_definitions.contains(name);
        self.regions
            .enclosing(region)
            .filter(move |_| !defined_here)
            .filter_map(|enclosing| self.globs.get(&enclosing))
            .map(Vec::as_slice)
    }

    fn bind_module(&mut self, region: RegionId, name: &str, module: BoundModule) {
        self.modules
            .entry(region)
            .or_default()
            .insert(name.to_string(), module);
    }

    fn bind_item(&mut self, region: RegionId, name: &str, dep: DependencyRef) {
        self.items
            .entry(region)
            .or_default()
            .insert(name.to_string(), dep);
    }

    fn bind_glob(&mut self, region: RegionId, dep: DependencyRef) {
        self.globs.entry(region).or_default().push(dep);
    }

    /// Bind the child modules of the module the glob `dep` imports from, and
    /// the names that module binds to a module by `use`, each where the file
    /// sees it from its crate, root and module.
    fn bind_glob_modules(
        &mut self,
        region: RegionId,
        ctx: &ResolutionContext,
        dep: &DependencyRef,
    ) {
        let viewer = ctx.viewer_for(dep);
        let bindings = viewer.bindings_in(CrateModule::of(dep));
        let module_paths = ctx.all_module_paths.get_or_empty(&dep.target_package);
        let children = module_paths.iter().filter_map(|path| {
            let (parent, name) = path.rsplit_once("::").unwrap_or(("", path));
            if parent != dep.target_module {
                return None;
            }
            let seen = bindings.is_none_or(|bindings| sees_the_module(bindings, name));
            if !seen {
                return None;
            }
            let child = BoundModule {
                package: dep.target_package.clone(),
                path: path.clone(),
                target_root: dep.target_root,
                import: None,
            };
            Some((name.to_string(), child))
        });
        let used_modules = bindings
            .into_iter()
            .flat_map(SeenBindings::all)
            .filter_map(|(name, binding)| Some((name, binding.use_target()?.0)))
            .filter(|(_, target)| module_is_visible(viewer, target))
            .filter_map(|(name, target)| {
                let bound = BoundModule {
                    package: target.crate_name.clone(),
                    path: append_to_path(&target.module, &target.original_name),
                    target_root: dep.target_root,
                    import: None,
                };
                if !ctx
                    .all_module_paths
                    .get_or_empty(&bound.package)
                    .contains(&bound.path)
                {
                    return None;
                }
                Some((name.to_string(), bound))
            });
        self.glob_modules
            .entry(region)
            .or_default()
            .extend(children.chain(used_modules));
    }

    /// Record the names of the items the file defines, so no glob claims them.
    fn define_locally(&mut self, syntax: &syn::File) {
        struct DefinedNames(HashSet<String>);
        impl<'ast> Visit<'ast> for DefinedNames {
            fn visit_item(&mut self, node: &'ast syn::Item) {
                let ident = match node {
                    syn::Item::Fn(i) => Some(&i.sig.ident),
                    syn::Item::Struct(i) => Some(&i.ident),
                    syn::Item::Enum(i) => Some(&i.ident),
                    syn::Item::Union(i) => Some(&i.ident),
                    syn::Item::Trait(i) => Some(&i.ident),
                    syn::Item::Type(i) => Some(&i.ident),
                    syn::Item::Const(i) => Some(&i.ident),
                    syn::Item::Static(i) => Some(&i.ident),
                    syn::Item::Mod(i) => Some(&i.ident),
                    syn::Item::Macro(i) => i.ident.as_ref(),
                    _ => None,
                };
                if let Some(ident) = ident {
                    self.0.insert(ident.to_string());
                }
                syn::visit::visit_item(self, node);
            }
        }
        let mut names = DefinedNames(HashSet::new());
        names.visit_file(syntax);
        self.local_definitions.extend(names.0);
    }

    fn bind_elsewhere(&mut self, region: RegionId, name: &str) {
        self.elsewhere
            .entry(region)
            .or_default()
            .insert(name.to_string());
    }
}

/// Each name the `use` items bind, with the `use` and the path it imports. A
/// glob binds the name `*`.
fn use_bindings(
    use_items: &CollectedUses,
) -> impl Iterator<Item = (&CollectedUse, String, String)> {
    use_items.iter().flat_map(|collected| {
        let alias_paths = resolve_use_tree(&collected.item.tree, "", true);
        let original_paths = resolve_use_tree(&collected.item.tree, "", false);
        alias_paths.into_iter().zip(original_paths).filter_map(
            move |(alias_path, original_path)| {
                let binding = alias_path.rsplit("::").next()?.to_string();
                Some((collected, binding, original_path))
            },
        )
    })
}

/// What one `use` binding means for later references to its name.
enum Binding {
    /// Names a module. A later `binding::Item` resolves under this module.
    Module(BoundModule),
    /// Names something this crate does not contain.
    Elsewhere,
    /// Names an item of this crate. A qualified use of an item is a reference to
    /// the item and already resolved, so later paths resolve on their own.
    OwnItem,
}

/// Classify one binding. `dep` is what its path resolved to, `None` where the
/// resolution chain could not place it.
///
/// A path the parser cannot place is a path out of this crate: every form that
/// stays inside resolves. Its name is therefore bound elsewhere, and that is what
/// keeps `parse_bare_module_import` off it.
///
/// A binding to a module, or to the root of another crate, is a module binding:
/// what follows the name resolves under that module.
///
/// A binding from another crate to an item needs an entry only where its name
/// also names a module of this crate. Without one, the reference lands on that
/// module. Whether it would is asked of `parse_bare_module_import` rather than
/// restated here, so the two cannot drift apart.
fn classify_binding(
    ctx: &ResolutionContext,
    dep: Option<&DependencyRef>,
    binding: &str,
) -> Binding {
    let Some(dep) = dep else {
        return Binding::Elsewhere;
    };
    if dep.target_item.is_none() {
        return Binding::Module(BoundModule::of(dep));
    }
    if dep.target_package == normalize_crate_name(ctx.current_crate) {
        return Binding::OwnItem;
    }
    if parse_bare_module_import(ctx, binding, 0, &EdgeContext::production()).is_some() {
        return Binding::Module(BoundModule::of(dep));
    }
    Binding::Elsewhere
}

/// `dep` followed through the re-export chain, so that a `use` of a name an
/// ancestor binds to a module is classified as the module. Where the chain
/// leads out of the crate, `dep` as it resolved.
fn through_the_chain(
    ctx: &ResolutionContext,
    dep: Option<&DependencyRef>,
) -> Option<DependencyRef> {
    let dep = dep?;
    let mut chained = dep.clone();
    Some(
        if ctx
            .resolve_reexport(&mut chained, Meaning::Any)
            .places_no_definer()
        {
            dep.clone()
        } else {
            chained
        },
    )
}

/// The module a binding names, by the package holding it, its path from the
/// package's root, and the root it resolves against. A binding of another
/// crate's item whose name also names a module of this crate holds the item's
/// path.
#[derive(Debug)]
struct BoundModule {
    package: String,
    path: String,
    target_root: TargetRoot,
    /// The [`DependencyRef::full_target`] of the `use` of the file that binds
    /// the name, if one does.
    import: Option<String>,
}

impl BoundModule {
    fn of(dep: &DependencyRef) -> BoundModule {
        let path = match &dep.target_item {
            Some(item) => append_to_path(&dep.target_module, item),
            None => dep.target_module.clone(),
        };
        BoundModule {
            package: dep.target_package.clone(),
            path,
            target_root: dep.target_root,
            import: None,
        }
    }

    /// Resolve `rest`, what a path writes after the bound name, under this
    /// module. A module of this crate's own tree resolves as a `crate::` path.
    /// Another package's, or the lib's from the bin tree, is looked up in that
    /// package, so this crate needs no name for it, and a name it has that
    /// means another package plays no part.
    fn resolve(
        &self,
        ctx: &ResolutionContext,
        rest: &str,
        line_num: usize,
        context: &EdgeContext,
        inline_depth: usize,
        meaning: Meaning,
    ) -> Option<DependencyRef> {
        let path = append_to_path(&self.path, rest);
        if self.target_root == TargetRoot::AsSeen
            && self.package == normalize_crate_name(ctx.current_crate)
        {
            let path = format!("crate::{path}");
            return resolve_single_path(ctx, &path, line_num, context, inline_depth, meaning);
        }
        let mut dep = resolve_in_package(ctx, &self.package, &path, line_num, context)?;
        take_item_the_parent_reexports(ctx, &mut dep, meaning);
        Some(dep)
    }
}

/// Parse path references into workspace-relevant dependencies, each carrying
/// the use the reference is.
///
/// Takes pre-collected path refs from `collect_all_path_refs()` and asks the
/// file's bindings what each stands for ([`FileBindings::binding_of`]). An
/// occurrence of a bound item is that item's dependency again; any other path
/// resolves through the existing resolution chain (`resolve_single_path()`).
/// The binding is authoritative, so a path it rules out is dropped rather than
/// resolved on its own.
/// Deduplicates by `full_target()` — same strategy as `parse_workspace_dependencies()`.
/// What the chains pass through is noted in `passed_on`. Beside the
/// dependencies, the [`BoundModule::import`] of each module binding a path
/// resolved through, and whether production code did.
pub(crate) fn parse_path_ref_dependencies(
    paths: &CollectedPathRefs,
    ctx: &ResolutionContext,
    bindings: &FileBindings,
    passed_on: &mut PassedOn,
) -> (Vec<DependencyRef>, HashMap<String, FromProduction>) {
    let mut used_module_imports: HashMap<String, FromProduction> = HashMap::new();
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashMap<(String, UsageKind), usize> = HashMap::new();
    // The globs a name was taken from, by `(full_target, kind)` of the glob.
    let mut used_globs: HashSet<(String, UsageKind)> = HashSet::new();
    // The bare identifier patterns that matched an item and so declared no
    // variable, by their index among the path references.
    let mut matching_patterns: HashSet<usize> = HashSet::new();

    for (index, path_ref) in paths.iter().enumerate() {
        let names_a_variable = match path_ref.local {
            None => false,
            Some(LocalBinding::Variable) => true,
            Some(LocalBinding::Pattern(pattern)) => !matching_patterns.contains(&pattern),
        };
        if names_a_variable {
            continue;
        }
        let mut module_import = None;
        let (mut dep, associated, glob_taken_from) =
            match bindings.binding_of(&path_ref.path, path_ref.region) {
                PathBinding::Unbound(name, associated) => {
                    // A qualified path resolves on its own where it can; what
                    // does not, and a bare name, is a glob's where a glob
                    // exports it.
                    if let Some(dep) = associated.and_then(|_| path_ref.resolve(ctx)) {
                        let associated = associated_segment(&path_ref.path, &dep);
                        (dep, associated, None)
                    } else {
                        let Some(glob) = glob_taking(ctx, bindings, name, path_ref.region) else {
                            continue;
                        };
                        let dep = DependencyRef {
                            target_item: Some(name.to_string()),
                            context: context_of_use(&path_ref.context, &glob.context),
                            ..glob.clone()
                        };
                        let taken_from = (glob.full_target(), dep.context.kind);
                        (dep, associated, Some(taken_from))
                    }
                }
                // A qualified path may still name a module beside the file: a
                // `use` of a fn `helper` leaves the module `helper` addressable,
                // the two being in different namespaces. The module wins where
                // it resolves. An occurrence of a name a `pub use` binds is a
                // use of the item, not part of the re-export.
                PathBinding::Item(bound, associated) => {
                    if let Some(dep) = associated.and_then(|_| path_ref.resolve(ctx)) {
                        let associated = associated_segment(&path_ref.path, &dep);
                        (dep, associated, None)
                    } else {
                        let dep = DependencyRef {
                            via_reexport: false,
                            context: context_of_use(&path_ref.context, &bound.context),
                            ..bound.clone()
                        };
                        (dep, associated, None)
                    }
                }
                PathBinding::Module(module, rest) => {
                    let Some(dep) = path_ref.resolve_under(ctx, module, rest) else {
                        continue;
                    };
                    module_import = module.import.as_deref();
                    let associated = associated_segment(&path_ref.path, &dep);
                    (dep, associated, None)
                }
                PathBinding::Elsewhere => continue,
            };
        let meaning = path_ref.meaning_of_item(associated);
        if path_ref.position == PathPosition::Binding && declares_a_variable(ctx, &dep, meaning) {
            continue;
        }
        if !ctx.resolve_noting(&mut dep, meaning, passed_on) {
            continue;
        }
        if let Some(import) = module_import {
            *used_module_imports.entry(import.to_string()).or_default() |=
                dep.context.kind == UsageKind::Production;
        }
        let Some(item) = dep.target_item.as_deref() else {
            DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
            continue;
        };
        if path_ref.position == PathPosition::Binding {
            matching_patterns.insert(index);
        }
        if let Some(glob) = glob_taken_from {
            used_globs.insert(glob);
        }
        let symbol_use = SymbolUse {
            line: path_ref.line,
            category: categorize(ctx, &dep, item, path_ref.position, associated, meaning),
            imported_for_methods: false,
        };
        dep.uses.push(symbol_use);
        DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
    }

    // A glob the file takes no name from keeps its `*`: the import is there,
    // and the edge with it. A re-export glob already carries its payload.
    for glob in bindings.globs.values().flatten() {
        if !glob.via_reexport && !used_globs.contains(&(glob.full_target(), glob.context.kind)) {
            DependencyRef::dedup_push(&mut deps, &mut seen_targets, glob.clone());
        }
    }

    (deps, used_module_imports)
}

/// The context of a name used through a binding: a test context where the
/// use or the binding is test code, with the use's test kind where both are.
fn context_of_use(reference: &EdgeContext, binding: &EdgeContext) -> EdgeContext {
    if matches!(reference.kind, UsageKind::Test(_)) {
        reference.clone()
    } else {
        binding.clone()
    }
}

/// The glob import of `bindings` that brings `name`, standing in `region`,
/// into scope: the innermost that brings it. A glob that brings it only
/// through a `use` under a `#[cfg]` syn cannot decide gives way to a glob of
/// the same scope that brings it in every build: where the `use` is in the
/// build, the name is ambiguous, so valid code names it only where the `use`
/// is not. `None` where a glob of another module brings it too and the module
/// of one of them declares `name` with different visibilities in its
/// alternative files: the file a build reads may hold it privately, leaving
/// it to another glob.
fn glob_taking<'b>(
    ctx: &ResolutionContext,
    bindings: &'b FileBindings,
    name: &str,
    region: RegionId,
) -> Option<&'b DependencyRef> {
    let brings = |glob: &DependencyRef| glob_exports(ctx, glob, name).is_some();
    let brings_in_every_build =
        |glob: &DependencyRef| glob_exports(ctx, glob, name) == Some(Brought::InEveryBuild);
    let (scope, first) = bindings
        .glob_scopes_for(name, region)
        .find_map(|scope| Some((scope, scope.iter().find(|glob| brings(glob))?)))?;
    let taken = if brings_in_every_build(first) {
        first
    } else {
        scope
            .iter()
            .find(|glob| brings_in_every_build(glob))
            .unwrap_or(first)
    };
    let differs = |glob: &DependencyRef| {
        ctx.target_bindings(glob)
            .is_some_and(|bindings| definition_visibilities_differ(bindings.every(name)))
    };
    let mut others = bindings
        .glob_scopes_for(name, region)
        .flatten()
        .filter(|glob| glob.full_target() != taken.full_target() && brings(glob))
        .peekable();
    let contested = others.peek().is_some() && (differs(taken) || others.any(differs));
    (!contested).then_some(taken)
}

/// Whether the module a glob imports from brings `name` into this file's
/// scope, and in which builds, as [`module_exports_symbol`] decides for the
/// file's module.
fn glob_exports(ctx: &ResolutionContext, glob: &DependencyRef, name: &str) -> Option<Brought> {
    module_exports_symbol(
        ctx.viewer_for(glob),
        CrateModule::of(glob),
        name,
        &mut HashSet::new(),
    )
}

/// Take the segment a path writes after the item it resolved to:
/// `crate::a::S::new` on a dependency on `S` leaves `new`. A path ending in the
/// item leaves nothing.
fn associated_segment<'p>(path: &'p str, dep: &DependencyRef) -> Option<&'p str> {
    let last = path.rsplit("::").next()?;
    (Some(last) != dep.target_item.as_deref()).then_some(last)
}

/// Whether the definitions among `named`, the bindings of one name, give it
/// different visibilities: alternatives under `cfg` in one file or in
/// alternative files.
fn definition_visibilities_differ<'a>(named: impl Iterator<Item = &'a NameBinding>) -> bool {
    let mut visibilities = named
        .filter(|binding| binding.definition().is_some())
        .map(|binding| &binding.visibility);
    visibilities
        .next()
        .is_some_and(|first| visibilities.any(|visibility| visibility != first))
}

/// The kind of the definition of `item` in the module `dep` points at: the
/// one in the namespace `meaning` asks, else any.
fn definition_kind(
    ctx: &ResolutionContext,
    dep: &DependencyRef,
    item: &str,
    meaning: Meaning,
) -> Option<DefKind> {
    let bindings = ctx.target_bindings(dep)?;
    let mut definitions = bindings
        .every(item)
        .filter(|binding| binding.definition().is_some());
    definitions
        .clone()
        .find(|binding| {
            binding
                .namespace()
                .is_some_and(|namespace| meaning.admits(namespace))
        })
        .or_else(|| definitions.next())
        .and_then(NameBinding::definition)
        .map(|definition| definition.kind)
}

/// Whether a bare identifier pattern naming `dep` declares a variable, which
/// takes no name through an ancestor. The chain of the probe goes unnoted, so
/// only a pattern that matches an item notes it. The pattern asks `meaning`
/// of the name.
fn declares_a_variable(ctx: &ResolutionContext, dep: &DependencyRef, meaning: Meaning) -> bool {
    let mut probed = dep.clone();
    !ctx.resolve_reexport(&mut probed, meaning)
        .places_no_definer()
        && probed.target_item.is_some()
        && !is_matched_by_a_bare_pattern(ctx, &probed)
}

/// Whether a bare identifier pattern matches `dep`'s item rather than
/// declaring a variable of that name: only a const, a static or a unit struct
/// can be matched that way. An item the map does not know is taken for a
/// variable, since a variable is what such a pattern nearly always declares.
fn is_matched_by_a_bare_pattern(ctx: &ResolutionContext, dep: &DependencyRef) -> bool {
    let Some(item) = &dep.target_item else {
        return false;
    };
    matches!(
        definition_kind(ctx, dep, item, Meaning::Value),
        Some(DefKind::Const | DefKind::Static | DefKind::Struct)
    )
}

/// Categorize one use by what its position takes from the item and, where the
/// position leaves that open, by what the item is in the namespace `meaning`
/// asks.
fn categorize(
    ctx: &ResolutionContext,
    dep: &DependencyRef,
    item: &str,
    position: PathPosition,
    associated: Option<&str>,
    meaning: Meaning,
) -> UseCategory {
    let kind = definition_kind(ctx, dep, item, meaning);
    let items = ctx
        .target_bindings(dep)
        .and_then(|bindings| bindings.associated(item));
    match (associated, position) {
        (_, PathPosition::Type | PathPosition::Construct) => UseCategory::Types,
        (Some(name), _) if items.is_some_and(|items| items.variants.contains(name)) => {
            UseCategory::Types
        }
        (Some(name), _) if items.is_some_and(|items| items.fns.contains(name)) => UseCategory::Fns,
        (Some(name), _) if items.is_some_and(|items| items.consts.contains(name)) => {
            UseCategory::Values
        }
        (Some(name), PathPosition::Call) => category_of_unknown_call(name),
        (Some(name), PathPosition::Value | PathPosition::Binding) => category_by_convention(name),
        (None, PathPosition::Call) => match kind {
            Some(DefKind::Struct | DefKind::Enum) => UseCategory::Types,
            Some(_) => UseCategory::Fns,
            None => category_of_unknown_call(item),
        },
        (None, PathPosition::Value | PathPosition::Binding) => match kind {
            Some(DefKind::Const | DefKind::Static) => UseCategory::Values,
            Some(DefKind::Fn) => UseCategory::Fns,
            Some(DefKind::Struct | DefKind::Enum | DefKind::Type | DefKind::Trait) => {
                UseCategory::Types
            }
            Some(DefKind::Macro) | None => category_by_convention(item),
        },
    }
}

/// Categorize a name by its spelling, where no definition says what it is:
/// Rust names constants in upper case, types in camel case and functions in
/// snake case.
fn category_by_convention(name: &str) -> UseCategory {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {
            if chars.all(|c| !c.is_lowercase()) {
                UseCategory::Values
            } else {
                UseCategory::Types
            }
        }
        _ => UseCategory::Fns,
    }
}

/// Categorize a call of a name no definition explains: capitalised, it
/// constructs a tuple struct or a variant; otherwise it calls a fn.
fn category_of_unknown_call(name: &str) -> UseCategory {
    if name.starts_with(char::is_uppercase) {
        UseCategory::Types
    } else {
        UseCategory::Fns
    }
}

/// Parse every dependency one file writes, with the uses each carries.
///
/// The `use` items name the imports; the path references say where and how
/// each imported name, and each qualified path, is used. Both are resolved
/// against the same bindings and deduplicated by `(full_target, kind)`, the
/// `use` items first so a location stays at the import line. What the
/// re-export chains of the dependencies pass through is noted in
/// `passed_on`. Without `include_tests`, the file's test code is not
/// resolved and yields no dependency.
pub(crate) fn parse_file_dependencies(
    syntax: &syn::File,
    ctx: &ResolutionContext,
    base_context: EdgeContext,
    include_tests: bool,
    passed_on: &mut PassedOn,
) -> Vec<DependencyRef> {
    let use_items = collect_all_use_items(syntax, base_context.clone());
    let path_refs = collect_all_path_refs(syntax, base_context);
    let is_production = |context: &EdgeContext| context.kind == UsageKind::Production;
    let holds_test_code = use_items
        .iter()
        .any(|collected| !is_production(&collected.context))
        || path_refs
            .iter()
            .any(|path_ref| !is_production(&path_ref.context));
    if ctx.reexport_map.is_test_build() || !holds_test_code {
        return resolve_file_dependencies(
            syntax, ctx, &use_items, &use_items, &path_refs, passed_on,
        );
    }
    // Test code of the file is compiled in the root's test build: its
    // references resolve there, and the production code's in the build
    // without `test`, which binds none of the test code's `use` items. Each
    // pass notes in `passed_on` only what it keeps.
    let production_uses = use_items.retaining(is_production);
    let mut deps = resolve_file_dependencies(
        syntax,
        ctx,
        &production_uses,
        &production_uses,
        &path_refs.retaining(is_production),
        passed_on,
    );
    if !include_tests {
        return deps;
    }
    let test_build = ctx.reexport_map.test_build();
    let test_ctx = ResolutionContext {
        reexport_map: test_build.as_ref().unwrap_or(ctx.reexport_map),
        ..*ctx
    };
    let test_deps = resolve_file_dependencies(
        syntax,
        &test_ctx,
        &use_items,
        &use_items.retaining(|context| !is_production(context)),
        &path_refs.retaining(|context| !is_production(context)),
        passed_on,
    );
    // A glob in production code that no test reference takes a name from
    // comes back from the second pass with its `*`; the first pass decides it.
    let mut seen = DependencyRef::build_seen_index(&deps);
    for dep in test_deps
        .into_iter()
        .filter(|dep| !is_production(&dep.context))
    {
        DependencyRef::dedup_push(&mut deps, &mut seen, dep);
    }
    deps
}

/// Resolve the `use` items `dep_uses` and the references `path_refs` of one
/// file against the bindings that `bound_uses` give the file, noting in
/// `passed_on` what their chains pass through.
fn resolve_file_dependencies(
    syntax: &syn::File,
    ctx: &ResolutionContext,
    bound_uses: &CollectedUses,
    dep_uses: &CollectedUses,
    path_refs: &CollectedPathRefs,
    passed_on: &mut PassedOn,
) -> Vec<DependencyRef> {
    let bindings = FileBindings::of(syntax, bound_uses, ctx);
    let use_deps = parse_workspace_dependencies(dep_uses, ctx, &bindings, passed_on);
    let (path_deps, used_module_imports) =
        parse_path_ref_dependencies(path_refs, ctx, &bindings, passed_on);

    // A private `use` of a module whose name a path resolved through carries
    // no symbol of its own; the uses through it carry theirs.
    let use_deps: Vec<DependencyRef> = use_deps
        .into_iter()
        .filter(|dep| {
            dep.target_item.is_some()
                || dep.via_reexport
                || !covers(used_module_imports.get(&dep.full_target()), dep)
        })
        .collect();
    let mut seen = DependencyRef::build_seen_index(&use_deps);
    let mut deps = use_deps;
    for dep in path_deps {
        DependencyRef::dedup_push(&mut deps, &mut seen, dep);
    }
    for dep in &mut deps {
        if dep.uses.is_empty()
            && !dep.via_reexport
            && let Some(item) = dep.target_item.as_deref()
            && item != "*"
        {
            let symbol_use = import_only_use(ctx, dep, item);
            dep.uses.push(symbol_use);
        }
    }
    deps
}

/// Derive the use of an import nothing in the file names. A trait is then
/// there for its methods, which method-call syntax never spells out. Any other
/// item is a dead import, and the use is what the definition says it is. A
/// re-export republishes the name and uses nothing, so it gets no use.
fn import_only_use(ctx: &ResolutionContext, dep: &DependencyRef, item: &str) -> SymbolUse {
    let is_trait = definition_kind(ctx, dep, item, Meaning::Type) == Some(DefKind::Trait);
    if is_trait {
        SymbolUse {
            line: dep.line,
            category: UseCategory::Fns,
            imported_for_methods: true,
        }
    } else {
        SymbolUse {
            line: dep.line,
            category: categorize(ctx, dep, item, PathPosition::Value, None, Meaning::Any),
            imported_for_methods: false,
        }
    }
}

/// Convenience wrapper: parse source text and extract dependencies.
/// Used by hir.rs which has source text but no pre-parsed AST.
#[cfg(feature = "hir")]
pub(crate) fn parse_workspace_dependencies_from_source(
    source: &str,
    ctx: &ResolutionContext,
) -> Vec<DependencyRef> {
    let syntax = match syn::parse_file(source) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    parse_file_dependencies(
        &syntax,
        ctx,
        EdgeContext::production(),
        true,
        &mut PassedOn::default(),
    )
}

#[cfg(test)]
mod tests;
