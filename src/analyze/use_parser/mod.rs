//! Syn-based use statement parsing for workspace dependency extraction.

use super::packages::{ModulePathMap, PackageExportMap, WorkspacePackages, normalize_crate_name};
use crate::model::{DefKind, Definition, DependencyRef, EntryKind, TargetRoot, UsageKind};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;
use syn::UseTree;

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
    pub(crate) fn of(kind: DefKind) -> Self {
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
    /// A `pub use`, `pub(crate) use` or `pub(super) use` of a path that names
    /// something outside the workspace, as [`BindingKind::Elsewhere`] does
    /// for a private `use`. The module is the last of the workspace on the
    /// way to the item, so a reference through it ends on it. It holds the
    /// path as the `use` writes it.
    ReExportFromOutside(String),
    /// A private, `pub(self)` or `pub(in …)` `use`. Rust makes it visible to
    /// descendant modules, which may then name the symbol through this module
    /// even though it defines nothing. Resolving through it attributes the
    /// edge to the definer, not to this ancestor.
    PrivateUse(ReExportTarget),
    /// A private `use` of a path [`UseScope::place`] finds no start for, or
    /// of a path from a name in scope that
    /// [`ReExportMap::settle_written_paths`] finds no module for: an
    /// external crate, which the map does not know. Nothing here names a
    /// definer, so a descendant's reference through this module
    /// produces no edge rather than staying on the ancestor.
    Elsewhere,
    /// A child module declared here. A glob or a path from where the module
    /// is not visible does not reach it.
    Module,
}

impl BindingKind {
    /// The place of the kind in the order of [`ModuleExportInfo::named`].
    fn rank(&self) -> u8 {
        match self {
            Self::Definition(_) => 0,
            Self::ReExport(_) => 1,
            Self::ReExportFromOutside(_) => 2,
            Self::PrivateUse(_) => 3,
            Self::Elsewhere => 4,
            Self::Module => 5,
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
    pub(crate) fn namespace(&self) -> Option<Namespace> {
        match &self.kind {
            BindingKind::Definition(definition) => Some(Namespace::of(definition.kind)),
            BindingKind::Module => Some(Namespace::Type),
            BindingKind::ReExport(_)
            | BindingKind::ReExportFromOutside(_)
            | BindingKind::PrivateUse(_)
            | BindingKind::Elsewhere => None,
        }
    }

    /// The definition the binding names, where it is one.
    pub(crate) fn definition(&self) -> Option<&Definition> {
        match &self.kind {
            BindingKind::Definition(definition) => Some(definition),
            _ => None,
        }
    }

    /// Whether a re-export chain that reaches the binding ends on its module:
    /// the module defines the name, or passes on an item from outside the
    /// workspace. A macro ends only a chain that asks for one, so a call of
    /// a fn of the same name goes on to the glob that brings it.
    fn ends_the_chain(&self, meaning: Meaning) -> bool {
        self.answers(meaning)
            && matches!(
                self.kind,
                BindingKind::Definition(_) | BindingKind::ReExportFromOutside(_)
            )
    }

    /// Whether the binding may be what a reference of `meaning` names: a
    /// macro answers only a reference that may name one.
    fn answers(&self, meaning: Meaning) -> bool {
        !matches!(
            &self.kind,
            BindingKind::Definition(definition)
                if definition.kind == DefKind::Macro && !meaning.admits(Namespace::Macro)
        )
    }

    /// The target of a `use`, public or private.
    pub(crate) fn use_target(&self) -> Option<&ReExportTarget> {
        match &self.kind {
            BindingKind::ReExport(target) | BindingKind::PrivateUse(target) => Some(target),
            BindingKind::Definition(_)
            | BindingKind::ReExportFromOutside(_)
            | BindingKind::Elsewhere
            | BindingKind::Module => None,
        }
    }

    /// The target of a `pub use`.
    pub(crate) fn reexported(&self) -> Option<&ReExportTarget> {
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
    /// The bindings of each name, in the order of [`ModuleExportInfo::named`].
    /// Alternatives under `cfg`, in one file or in alternative files, and
    /// bindings in different namespaces are each a binding of their own.
    bindings: HashMap<String, Vec<NameBinding>>,
    /// The associated items of the types and traits defined here, by the
    /// defined name. An `impl` block in another file of the crate lands here
    /// too, on the module that defines the type.
    pub(crate) associated: HashMap<String, AssociatedItems>,
    /// The globs of the module: `pub use` globs of its own crate, of other
    /// workspace crates, then private globs of its own crate and of others.
    globs: Vec<GlobBinding>,
    /// The names a `use` binds by a path from a name in scope: its first
    /// name is neither a child module nor a crate, so it is a binding of the
    /// module or an external crate ([`UsePlace::Name`]).
    from_scope: HashSet<String>,
    /// Whether an item of the module calls a macro, or carries an attribute
    /// or derive macro rustc does not build in. What the macro expands to
    /// may define names no binding lists.
    pub(crate) expands_macros: bool,
    /// The visibility of each glob of the module whose path the module tree
    /// does not place, such as a glob of an external crate. Which names it
    /// brings, the map does not know.
    pub(crate) unplaced_globs: Vec<Visibility>,
    /// The names the module binds by an item no binding lists: a union, an
    /// `extern crate`, and in the crate root a `#[macro_export]` macro.
    pub(crate) unlisted: HashSet<String>,
}

/// A glob `use` as a module holds it: how far its names reach, the module it
/// imports from by crate and path, whether that crate is another workspace
/// crate than the holder's, and where the names it brings in are visible.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlobBinding {
    pub(crate) reach: GlobReach,
    pub(crate) crate_name: String,
    pub(crate) module: String,
    pub(crate) foreign: bool,
    pub(crate) visibility: Visibility,
}

impl GlobBinding {
    /// The place of the glob in the order of [`ModuleExportInfo::globs`].
    fn rank(&self) -> u8 {
        let reach = match self.reach {
            GlobReach::ReExport => 0,
            GlobReach::Private => 2,
        };
        reach + u8::from(self.foreign)
    }

    /// The module the glob imports from.
    fn source_module(&self) -> CrateModule<'_> {
        CrateModule {
            crate_name: &self.crate_name,
            path: &self.module,
        }
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
            && !self.expands_macros
            && self.unplaced_globs.is_empty()
            && self.unlisted.is_empty()
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

    /// Bind `name` by `binding` like [`ModuleExportInfo::bind`], for a `use`
    /// whose path starts from a name in scope.
    pub(crate) fn bind_from_scope(&mut self, name: String, binding: NameBinding) {
        self.from_scope.insert(name.clone());
        self.bind(name, binding);
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
        self.from_scope.extend(other.from_scope);
        self.expands_macros |= other.expands_macros;
        self.unplaced_globs.extend(other.unplaced_globs);
        self.unlisted.extend(other.unlisted);
    }

    /// The bindings of `name`: definitions, `pub use` items, `pub use` items
    /// of something outside the workspace, private `use` items, `use` items
    /// of something unplaced, child modules, in this
    /// order. Of one kind, the binding read last comes first, so a caller
    /// that takes one of several alternatives takes the one read last. The
    /// map is read once; a caller that asks several things of the bindings
    /// clones the iterator.
    pub(crate) fn named<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.bindings.get(name).into_iter().flatten()
    }

    /// Every binding of every name, grouped by kind in the order of
    /// [`ModuleExportInfo::named`].
    pub(crate) fn all(&self) -> impl Iterator<Item = (&str, &NameBinding)> {
        let bindings = &self.bindings;
        (0..=BindingKind::Module.rank()).flat_map(move |rank| {
            bindings.iter().flat_map(move |(name, bindings)| {
                bindings
                    .iter()
                    .filter(move |binding| binding.kind.rank() == rank)
                    .map(move |binding| (name.as_str(), binding))
            })
        })
    }

    /// The globs of the module, in the order the field keeps them.
    fn globs(&self) -> impl Iterator<Item = &GlobBinding> {
        self.globs.iter()
    }

    /// The target of each `use` binding, with its name, its place among the
    /// bindings of the name, and whether it is a `pub use`.
    fn use_targets(&self) -> impl Iterator<Item = (&str, usize, bool, &ReExportTarget)> {
        self.bindings.iter().flat_map(|(name, bindings)| {
            bindings
                .iter()
                .enumerate()
                .filter_map(move |(index, binding)| {
                    let target = binding.use_target()?;
                    let re_export = binding.reexported().is_some();
                    Some((name.as_str(), index, re_export, target))
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

/// The bindings of one module as a [`Viewer`] sees them, the answer to "name
/// N in module M, seen by V". Each lookup gives what the viewer sees; what it
/// does not see comes only from [`SeenBindings::unseen`] and
/// [`SeenBindings::every`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct SeenBindings<'a> {
    crate_name: &'a str,
    info: &'a ModuleExportInfo,
    viewer: Viewer<'a>,
}

impl<'a> SeenBindings<'a> {
    fn sees(self, visibility: &Visibility) -> bool {
        self.viewer.sees(self.crate_name, visibility)
    }

    /// The bindings of `name` the viewer sees, in the order of
    /// [`ModuleExportInfo::named`].
    pub(crate) fn named(
        self,
        name: &str,
    ) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.info
            .named(name)
            .filter(move |binding| self.sees(&binding.visibility))
    }

    /// The bindings of `name` the viewer does not see, in the same order.
    fn unseen(self, name: &str) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.info
            .named(name)
            .filter(move |binding| !self.sees(&binding.visibility))
    }

    /// The bindings of `name`, seen or not, for a fact about the name that
    /// holds whoever looks: whether its alternatives differ, or what kind its
    /// definition is.
    pub(crate) fn every(
        self,
        name: &str,
    ) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.info.named(name)
    }

    /// Every binding of every name the viewer sees, in the order of
    /// [`ModuleExportInfo::all`].
    pub(crate) fn all(self) -> impl Iterator<Item = (&'a str, &'a NameBinding)> {
        self.info
            .all()
            .filter(move |(_, binding)| self.sees(&binding.visibility))
    }

    /// The globs of the module the viewer sees, in the order of
    /// [`ModuleExportInfo::globs`].
    fn globs(self) -> impl Iterator<Item = &'a GlobBinding> {
        self.info
            .globs()
            .filter(move |glob| self.sees(&glob.visibility))
    }

    /// The associated items of the type or trait the module defines as
    /// `name`.
    pub(crate) fn associated(self, name: &str) -> Option<&'a AssociatedItems> {
        self.info.associated.get(name)
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

    /// Each module of the map, in every root and build, with its package,
    /// root, build and path.
    fn module_infos(
        &self,
    ) -> impl Iterator<Item = (&String, usize, bool, &String, &ModuleExportInfo)> {
        self.packages.iter().flat_map(|(package, roots)| {
            roots.iter().enumerate().flat_map(move |(root, exports)| {
                exports.builds().flat_map(move |(test_build, modules)| {
                    modules
                        .iter()
                        .map(move |(module, info)| (package, root, test_build, module, info))
                })
            })
        })
    }

    /// Each `use` binding of the map, in every root and build, with its place.
    fn use_targets(&self) -> impl Iterator<Item = (BindingPlace, &ReExportTarget)> {
        self.module_infos()
            .flat_map(|(package, root, test_build, module, info)| {
                info.use_targets()
                    .map(move |(name, index, re_export, target)| {
                        let place = BindingPlace {
                            package: package.clone(),
                            root,
                            test_build,
                            module: module.clone(),
                            name: name.to_string(),
                            index,
                            re_export,
                            from_scope: info.from_scope.contains(name),
                        };
                        (place, target)
                    })
            })
    }

    /// The module `module` of the `root`-th root of `package`, in its test
    /// build where `test_build` is set.
    fn module_mut(
        &mut self,
        package: &str,
        root: usize,
        test_build: bool,
        module: &str,
    ) -> Option<&mut ModuleExportInfo> {
        let exports = Arc::make_mut(&mut self.packages)
            .get_mut(package)?
            .get_mut(root)?;
        let modules = if test_build {
            exports.test_build_modules.as_mut()?
        } else {
            &mut exports.modules
        };
        modules.get_mut(module)
    }

    /// Settle the `use` binding at each place: point it at its target, or,
    /// where it names something outside the workspace, bind a private `use`
    /// [`BindingKind::Elsewhere`] and a `pub use`
    /// [`BindingKind::ReExportFromOutside`].
    fn rebind(&mut self, settled: Vec<(BindingPlace, Settled)>) {
        let mut outside: HashMap<(ModuleKey, String), HashMap<usize, BindingKind>> = HashMap::new();
        for (place, settled) in settled {
            let kind = match settled {
                Settled::Target(target) => {
                    let binding = self
                        .module_mut(&place.package, place.root, place.test_build, &place.module)
                        .and_then(|info| info.bindings.get_mut(&place.name))
                        .and_then(|bindings| bindings.get_mut(place.index));
                    if let Some(NameBinding {
                        kind: BindingKind::ReExport(held) | BindingKind::PrivateUse(held),
                        ..
                    }) = binding
                    {
                        *held = target;
                    }
                    continue;
                }
                Settled::Elsewhere => BindingKind::Elsewhere,
                Settled::ReExportFromOutside(path) => BindingKind::ReExportFromOutside(path),
            };
            let module = (place.package, place.root, place.test_build, place.module);
            outside
                .entry((module, place.name))
                .or_default()
                .insert(place.index, kind);
        }
        for (((package, root, test_build, module), name), mut kinds) in outside {
            let Some(info) = self.module_mut(&package, root, test_build, &module) else {
                continue;
            };
            let Some(bindings) = info.bindings.remove(&name) else {
                continue;
            };
            // Bound again oldest first, so the order of the kinds holds.
            for (index, mut binding) in bindings.into_iter().enumerate().rev() {
                if let Some(kind) = kinds.remove(&index) {
                    binding.kind = kind;
                }
                info.bind(name.clone(), binding);
            }
        }
    }

    /// Point each written glob at the module it reaches, or hold it as
    /// unplaced where it reaches none.
    fn reglob(&mut self, settled: Vec<(GlobPlace, NameTarget)>) {
        let mut by_module: HashMap<ModuleKey, HashMap<usize, NameTarget>> = HashMap::new();
        for (place, reached) in settled {
            let key = (place.package, place.root, place.test_build, place.module);
            by_module
                .entry(key)
                .or_default()
                .insert(place.index, reached);
        }
        for ((package, root, test_build, module), reached) in by_module {
            let Some(info) = self.module_mut(&package, root, test_build, &module) else {
                continue;
            };
            let globs = std::mem::take(&mut info.globs);
            for (index, mut glob) in globs.into_iter().enumerate() {
                match reached.get(&index) {
                    Some(NameTarget::Item) => {
                        info.unplaced_globs.push(glob.visibility);
                        continue;
                    }
                    Some(NameTarget::Module { crate_name, path }) => {
                        glob.foreign = *crate_name != package;
                        glob.crate_name.clone_from(crate_name);
                        glob.module.clone_from(path);
                    }
                    None => {}
                }
                info.hold_glob(glob);
            }
        }
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
    /// Whether the binding is a `pub use`.
    re_export: bool,
    /// Whether the `use` path of the binding starts from a name in scope.
    from_scope: bool,
}

/// A module of a [`ReExportMap`] by package, root, build and path.
type ModuleKey = (String, usize, bool, String);

/// Where a glob sits in a [`ReExportMap`]: package, root, build, module, and
/// place among the globs of the module.
struct GlobPlace {
    package: String,
    root: usize,
    test_build: bool,
    module: String,
    index: usize,
}

/// What a written `use` path settles to: the target it reaches, or, where a
/// path from a name in scope reaches nothing in the workspace, what its
/// binding becomes.
enum Settled {
    Target(ReExportTarget),
    /// A private `use` of something outside the workspace.
    Elsewhere,
    /// A `pub use` of something outside the workspace, with the path written
    /// from the module that holds it.
    ReExportFromOutside(String),
}

/// What a name in a module stands for, as a viewer sees it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NameTarget {
    /// A module of a workspace crate, to which a path carries its rest. Its
    /// path is `""` for the crate root.
    Module { crate_name: String, path: String },
    /// An item the module binds under the name, its own or through a `use`
    /// or a glob.
    Item,
}

/// What a written path reaches as far as the map tells now.
enum Reached {
    /// The target the path reaches.
    Target(ReExportTarget),
    /// The first name binds nothing in the module the path starts in.
    Unbound,
    /// A name's lookup entered a module that a path still written holds:
    /// what the name stands for is known once that path is settled.
    Waits,
}

/// Where the written part of a path starts, and what a name there that
/// stands for no module means.
#[derive(Clone, Copy)]
enum WrittenFrom<'a> {
    /// The root of the crate: a path into another workspace crate, or one
    /// of the own crate from `crate::`, `self::`, `super::` or a child
    /// module. A first name the root binds nothing under leaves the path
    /// [`Reached::Unbound`], an item ends it.
    CrateRoot,
    /// The module holding a `use` whose path starts from a name in scope: a
    /// first name that stands for no module there names an external crate,
    /// since a path goes on only through a module, and leaves the path
    /// [`Reached::Unbound`].
    Scope(&'a str),
}

/// The target the path `target` holds as written reaches from the root of
/// its crate, as `lookup` sees it: each name but the last carries the path
/// into the module it stands for, and the name that stands for no module
/// ends it, as an item whose associated item the rest names. A name that
/// binds nothing ends it the same way, except where the written part of the
/// path starts (`from`). A name its holder binds to alternatives under
/// `cfg` ends it on the holder: the target is the last name there, so a
/// lookup of it stays in the module that binds the alternatives.
/// `unsettled` holds the modules that paths still written hold.
fn reached_target(
    lookup: NameLookup,
    target: &ReExportTarget,
    from: WrittenFrom,
    unsettled: &HashSet<(String, String)>,
) -> Reached {
    let written: Vec<&str> = target
        .module
        .split("::")
        .chain(std::iter::once(target.original_name.as_str()))
        .collect();
    let Some((last, through)) = written.split_last() else {
        return Reached::Unbound;
    };
    let mut crate_name = target.crate_name.clone();
    let mut module = String::new();
    for name in through {
        let holder = CrateModule {
            crate_name: &crate_name,
            path: &module,
        };
        let at_start = crate_name == target.crate_name;
        match WrittenStep::of(lookup, holder, name, unsettled) {
            WrittenStep::Alternatives => {
                return Reached::Target(target_in(crate_name, module, last));
            }
            WrittenStep::Waits => return Reached::Waits,
            WrittenStep::Module {
                crate_name: reached_crate,
                path,
            } => {
                crate_name = reached_crate;
                module = path;
            }
            _ if at_start && matches!(from, WrittenFrom::Scope(start) if module == start) => {
                return match lookup.type_item_definer(holder, name, &mut Visited::default()) {
                    Some(definer) => Reached::Target(definer),
                    None => Reached::Unbound,
                };
            }
            WrittenStep::Unbound
                if at_start && module.is_empty() && matches!(from, WrittenFrom::CrateRoot) =>
            {
                return Reached::Unbound;
            }
            WrittenStep::Item | WrittenStep::Unbound => {
                return Reached::Target(target_in(crate_name, module, name));
            }
        }
    }
    Reached::Target(target_in(crate_name, module, last))
}

/// The module the written path of `glob` reaches from the root of its
/// crate, as `lookup` sees it, or [`NameTarget::Item`] where a name on it
/// stands for no module or for alternatives under `cfg`. `None` where a
/// name's lookup enters a module of `unsettled`.
fn reached_module(
    lookup: NameLookup,
    glob: &GlobBinding,
    unsettled: &HashSet<(String, String)>,
) -> Option<NameTarget> {
    let mut crate_name = glob.crate_name.clone();
    let mut module = String::new();
    for name in glob.module.split("::") {
        let holder = CrateModule {
            crate_name: &crate_name,
            path: &module,
        };
        match WrittenStep::of(lookup, holder, name, unsettled) {
            WrittenStep::Waits => return None,
            WrittenStep::Module {
                crate_name: reached_crate,
                path,
            } => {
                crate_name = reached_crate;
                module = path;
            }
            WrittenStep::Alternatives | WrittenStep::Item | WrittenStep::Unbound => {
                return Some(NameTarget::Item);
            }
        }
    }
    Some(NameTarget::Module {
        crate_name,
        path: module,
    })
}

/// What one name of a written path that goes on past it stands for in its
/// holder.
enum WrittenStep {
    /// Alternatives under `cfg` the holder binds, of which syn cannot tell
    /// which a build takes.
    Alternatives,
    /// Not known yet: the lookup entered a module a path still written
    /// holds.
    Waits,
    /// A module, which carries the path on.
    Module { crate_name: String, path: String },
    /// An item.
    Item,
    /// Nothing the holder binds.
    Unbound,
}

impl WrittenStep {
    /// What `name` in `holder` stands for, as `lookup` sees it, where
    /// `unsettled` holds the modules that paths still written hold.
    fn of(
        lookup: NameLookup,
        holder: CrateModule,
        name: &str,
        unsettled: &HashSet<(String, String)>,
    ) -> Self {
        if binds_alternatives(lookup, holder, name) {
            return Self::Alternatives;
        }
        let mut visited = Visited::default();
        let named = lookup.name_passing(holder, name, AfterName::PathGoesOn, &mut visited);
        if visited.entered_any(unsettled) {
            return Self::Waits;
        }
        match named {
            Some(NameTarget::Module { crate_name, path }) => Self::Module { crate_name, path },
            Some(NameTarget::Item) => Self::Item,
            None => Self::Unbound,
        }
    }
}

/// Whether `holder` binds `name` to different targets by private `use`
/// items that may carry a path on, as `lookup` sees it: alternatives under
/// `cfg`, of which syn cannot tell which a build takes. A target its module
/// defines only outside the type namespace carries no path on, so it is no
/// alternative to one that does.
fn binds_alternatives(lookup: NameLookup, holder: CrateModule, name: &str) -> bool {
    let Some(bindings) = lookup.viewer.bindings_in(holder) else {
        return false;
    };
    let mut targets = bindings
        .every(name)
        .filter_map(|binding| match &binding.kind {
            BindingKind::PrivateUse(target) => Some(target),
            _ => None,
        })
        .filter(|target| !lookup.defines_only_outside_the_type_namespace(target));
    let differ = targets
        .next()
        .is_some_and(|first| targets.any(|target| target != first));
    differ
        && bindings
            .named(name)
            .any(|binding| matches!(binding.kind, BindingKind::PrivateUse(_)))
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

impl ReExportMap {
    /// Point each `use` whose target holds the path as written, where
    /// [`UseScope::place`] left it, at what that path reaches, and each glob
    /// whose module is written at the module it reaches. Each path is read
    /// from the root of its crate: into another workspace crate as seen from
    /// outside, in the own crate as the module holding the `use` sees it. A
    /// path that runs through a binding or glob still written waits for a
    /// later round, and rounds repeat while one settles a path. A path whose
    /// first name the root binds nothing under names that name in the root.
    /// A path from a name in scope ([`UsePlace::Name`]) whose first
    /// name stands for no module in the holding module names an external
    /// crate: a private `use` of it binds [`BindingKind::Elsewhere`], a
    /// `pub use` [`BindingKind::ReExportFromOutside`]. A glob whose path does not reach a module
    /// imports nothing.
    pub(crate) fn settle_written_paths(mut self, module_paths: &ModulePathMap) -> Self {
        loop {
            let unsettled = self.unsettled(module_paths);
            let targets = self.settled_targets(module_paths, &unsettled);
            let globs = self.settled_globs(module_paths, &unsettled);
            if targets.is_empty() && globs.is_empty() {
                break;
            }
            self.rebind(targets);
            self.reglob(globs);
        }
        let unreached: Vec<_> = self
            .written_targets(module_paths)
            .filter(|(place, _)| !place.from_scope)
            .map(|(place, target)| (place, Settled::Target(root_name_of(target))))
            .collect();
        self.rebind(unreached);
        let nothing_waits = HashSet::new();
        let targets = self.settled_targets(module_paths, &nothing_waits);
        self.rebind(targets);
        let globs = self.settled_globs(module_paths, &nothing_waits);
        self.reglob(globs);
        self
    }

    /// The modules that paths still written hold, by crate and path: of
    /// `use` targets and of globs.
    fn unsettled(&self, module_paths: &ModulePathMap) -> HashSet<(String, String)> {
        let targets = self
            .written_targets(module_paths)
            .map(|(_, target)| (target.crate_name.clone(), target.module.clone()));
        let globs = self
            .written_globs(module_paths)
            .map(|(_, glob)| (glob.crate_name.clone(), glob.module.clone()));
        targets.chain(globs).collect()
    }

    /// What each written `use` target settles to, where its path waits on
    /// none of `unsettled`: the target it reaches, or, for a path from a name
    /// in scope that reaches nothing, [`Settled::Elsewhere`] for a private
    /// `use` and [`Settled::ReExportFromOutside`] for a `pub use`. A path not
    /// from a name in scope whose first name the root binds nothing under
    /// stays written.
    fn settled_targets(
        &self,
        module_paths: &ModulePathMap,
        unsettled: &HashSet<(String, String)>,
    ) -> Vec<(BindingPlace, Settled)> {
        self.written_targets(module_paths)
            .filter_map(|(place, target)| {
                let own = target.crate_name == place.package;
                let own_root = (place.package.clone(), place.root);
                let lookup = NameLookup {
                    viewer: self.viewer_of_written(own, &own_root, place.test_build, &place.module),
                    module_paths,
                };
                let from = if place.from_scope {
                    WrittenFrom::Scope(&place.module)
                } else {
                    WrittenFrom::CrateRoot
                };
                let settled = match reached_target(lookup, target, from, unsettled) {
                    Reached::Target(target) => Settled::Target(target),
                    Reached::Unbound if place.from_scope => {
                        // Written from a name in scope, the path starts with
                        // the module that holds it.
                        if !place.re_export {
                            return Some((place, Settled::Elsewhere));
                        }
                        let path = append_to_path(&target.module, &target.original_name);
                        let from_holder = path
                            .strip_prefix(&format!("{}::", place.module))
                            .map_or(path.clone(), ToString::to_string);
                        Settled::ReExportFromOutside(from_holder)
                    }
                    Reached::Unbound | Reached::Waits => return None,
                };
                Some((place, settled))
            })
            .collect()
    }

    /// The module each written glob reaches, [`NameTarget::Item`] where its
    /// path reaches no module, where the path waits on none of `unsettled`.
    fn settled_globs(
        &self,
        module_paths: &ModulePathMap,
        unsettled: &HashSet<(String, String)>,
    ) -> Vec<(GlobPlace, NameTarget)> {
        self.written_globs(module_paths)
            .filter_map(|(place, glob)| {
                let own = glob.crate_name == place.package;
                let own_root = (place.package.clone(), place.root);
                let lookup = NameLookup {
                    viewer: self.viewer_of_written(own, &own_root, place.test_build, &place.module),
                    module_paths,
                };
                let reached = reached_module(lookup, glob, unsettled)?;
                Some((place, reached))
            })
            .collect()
    }

    /// The viewer that reads a path written in `module` of `own_root`, in
    /// its test build where `test_build` is set: from that module where the
    /// path stays in the own crate (`own`), else from outside every package.
    fn viewer_of_written<'a>(
        &'a self,
        own: bool,
        own_root: &'a (String, usize),
        test_build: bool,
        module: &'a str,
    ) -> Viewer<'a> {
        if !own {
            return self.viewer_from_outside();
        }
        Viewer {
            packages: &self.packages,
            own_root: Some(own_root),
            test_build,
            module: Some(module),
        }
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

    /// Each `use` binding whose target lies in no module of its crate: the
    /// path as written.
    fn written_targets<'a>(
        &'a self,
        module_paths: &'a ModulePathMap,
    ) -> impl Iterator<Item = (BindingPlace, &'a ReExportTarget)> + 'a {
        self.use_targets()
            .filter(move |(_, target)| !module_paths.holds(&target.crate_name, &target.module))
    }

    /// Each glob whose module lies in no module of its crate: the path as
    /// written.
    fn written_globs<'a>(
        &'a self,
        module_paths: &'a ModulePathMap,
    ) -> impl Iterator<Item = (GlobPlace, &'a GlobBinding)> + 'a {
        self.module_infos()
            .flat_map(|(package, root, test_build, module, info)| {
                info.globs.iter().enumerate().map(move |(index, glob)| {
                    let place = GlobPlace {
                        package: package.clone(),
                        root,
                        test_build,
                        module: module.clone(),
                        index,
                    };
                    (place, glob)
                })
            })
            .filter(move |(_, glob)| !module_paths.holds(&glob.crate_name, &glob.module))
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
pub(crate) struct Viewer<'a> {
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
    pub(crate) fn bindings_in(self, module: CrateModule) -> Option<SeenBindings<'a>> {
        let (crate_name, modules) = self.modules(module.crate_name)?;
        Some(SeenBindings {
            crate_name,
            info: modules.get(module.path)?,
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
pub(crate) struct CrateModule<'a> {
    pub(crate) crate_name: &'a str,
    pub(crate) path: &'a str,
}

impl<'a> CrateModule<'a> {
    /// The module `dep` points at.
    pub(crate) fn of(dep: &'a DependencyRef) -> Self {
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
pub(crate) type FromProduction = bool;

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
            let lookup = NameLookup {
                viewer: reexport_map.viewer_for(dep, holder),
                module_paths,
            };
            return dep.target_package == crate_name
                && covers(self.globs.get(&noted), dep)
                && !lookup.may_bring_a_trait(dep);
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
pub(crate) fn covers(note: Option<&FromProduction>, dep: &DependencyRef) -> bool {
    note.is_some_and(|&from_production| {
        from_production || dep.context.kind != UsageKind::Production
    })
}

/// Whether the only use of `dep` is the one `import_only_use` derives for an
/// import nothing in the file names, and it is no trait imported for its
/// methods.
fn is_import_only(dep: &DependencyRef) -> bool {
    matches!(
        dep.uses.as_slice(),
        [only] if only.line == dep.line && !only.imported_for_methods
    )
}

/// The modules a walk of a [`NameLookup`] already entered, by crate and module
/// path, with the name asked there where one walk asks several names.
#[derive(Debug, Default, Clone)]
pub(crate) struct Visited(HashSet<(String, String, String)>);

impl Visited {
    /// Record that a walk entered `module` asking `name`, `""` for a walk that
    /// asks one name throughout. Whether it had not before.
    fn first_entry(&mut self, module: CrateModule, name: &str) -> bool {
        self.0.insert((
            module.crate_name.to_string(),
            module.path.to_string(),
            name.to_string(),
        ))
    }

    /// Whether the walk entered one of `modules`, given by crate and module
    /// path.
    fn entered_any(&self, modules: &HashSet<(String, String)>) -> bool {
        self.0
            .iter()
            .any(|(crate_name, path, _)| modules.contains(&(crate_name.clone(), path.clone())))
    }
}

/// What a name lookup asks: what the name stands for, or only the module
/// it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Everything,
    Modules,
}

/// What a path does after the name it asks of a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AfterName {
    /// The name is the last segment of the path.
    PathEnds,
    /// More segments follow, which the name has to carry on.
    PathGoesOn,
}

impl AfterName {
    /// What the path does after a name `rest` follows.
    pub(crate) fn followed_by(rest: &[&str]) -> Self {
        if rest.is_empty() {
            Self::PathEnds
        } else {
            Self::PathGoesOn
        }
    }
}

/// The lookup of "what is name N in module M for viewer V": a viewer and the
/// module tree. Each question walks the binding table through
/// [`NameLookup::enter`], which reads a module with the viewer that reaches
/// its crate, records it in the walk's [`Visited`] and gives only the
/// bindings the viewer sees. No question calls [`Viewer::reaching`] or
/// records a module itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NameLookup<'a> {
    pub(crate) viewer: Viewer<'a>,
    module_paths: &'a ModulePathMap,
}

impl<'a> NameLookup<'a> {
    /// The same lookup, seen by `viewer`.
    fn seen_by(self, viewer: Viewer<'a>) -> Self {
        Self { viewer, ..self }
    }

    /// The lookup that reads the sources of the globs `holder` holds: a glob
    /// imports only what its holder sees ([`Viewer::through`]).
    fn through(self, holder: CrateModule) -> Self {
        self.seen_by(self.viewer.through(holder))
    }

    /// Enter `module` in a walk that has entered `visited`, asking `name`
    /// where the walk asks several names. Gives the lookup that reads on
    /// from the module, with the viewer that reaches its crate, and the
    /// module's bindings that viewer sees, `None` where the map holds no
    /// such module. `None` altogether where the walk entered the module
    /// before.
    fn enter(
        self,
        module: CrateModule,
        name: Option<&str>,
        visited: &mut Visited,
    ) -> Option<(Self, Option<SeenBindings<'a>>)> {
        let lookup = self.seen_by(self.viewer.reaching(module.crate_name));
        if !visited.first_entry(module, name.unwrap_or_default()) {
            return None;
        }
        Some((lookup, lookup.viewer.bindings_in(module)))
    }

    /// Walk `module` and the modules its globs of `reach`, or of any reach,
    /// import from, depth first in the order the modules hold their globs,
    /// until `visit` breaks. Each source is read through the module holding
    /// the glob. A module the walk entered before is not visited again.
    fn walk_globs<B>(
        self,
        module: CrateModule,
        reach: Option<GlobReach>,
        visited: &mut Visited,
        visit: &mut impl FnMut(Self, CrateModule, Option<SeenBindings<'a>>) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        let Some((lookup, bindings)) = self.enter(module, None, visited) else {
            return ControlFlow::Continue(());
        };
        visit(lookup, module, bindings)?;
        let globs = bindings
            .into_iter()
            .flat_map(SeenBindings::globs)
            .filter(|glob| reach.is_none_or(|reach| glob.reach == reach));
        for glob in globs {
            lookup
                .through(module)
                .walk_globs(glob.source_module(), reach, visited, visit)?;
        }
        ControlFlow::Continue(())
    }

    /// Follow the re-export chain of `dep` until the original definition
    /// module is found, crossing into another workspace crate where the chain
    /// does. Modifies `dep.target_package` and `dep.target_module` in place; a
    /// no-op if no re-export applies. The [`Resolution`] tells whether the
    /// chain instead reaches a name a private `use` bound to something outside
    /// the crate (an external crate), and which private `use` items and globs
    /// of other modules it passed through. Whether the reference means a
    /// module, `meaning` tells.
    fn follow(self, dep: &mut DependencyRef, meaning: Meaning) -> Resolution {
        let mut lookup = self;
        let mut lookup_name = dep.target_item.clone();
        let mut passed = Vec::new();
        let mut visited = Visited::default();

        while let Some((reached, bindings)) = lookup.enter(CrateModule::of(dep), None, &mut visited)
        {
            lookup = reached;
            let Some(bindings) = bindings else {
                break;
            };
            let Some(item) = &lookup_name else {
                break;
            };

            let named = bindings.named(item);
            if named.clone().any(|binding| binding.ends_the_chain(meaning)) {
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
            if let Some(binding) = binding
                && let Some(target) = binding.use_target()
            {
                if lookup.end_on_module(dep, target, meaning) {
                    break;
                }
                if matches!(binding.kind, BindingKind::PrivateUse(_)) {
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

            if lookup.follow_glob(dep, bindings, item, meaning, &visited, &mut passed) {
                continue;
            }

            // Tier 3: A name no binding lists, which only a macro can define.
            if let Some((crate_name, module)) =
                lookup.sole_macro_expander(CrateModule::of(dep), item)
            {
                dep.target_package = crate_name;
                dep.target_module = module;
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
    /// at, takes `symbol` from: a `pub use` glob of this crate first, then of
    /// another workspace crate, then a private glob, which reaches
    /// descendants only. A private glob of this crate is pushed onto
    /// `passed`. A `pub use` glob does not lead back into a module in
    /// `chain`, the modules the chain entered. Returns whether a glob brings
    /// the symbol as `meaning` asks for it.
    fn follow_glob(
        self,
        dep: &mut DependencyRef,
        bindings: SeenBindings,
        symbol: &str,
        meaning: Meaning,
        chain: &Visited,
        passed: &mut Vec<PassedThrough>,
    ) -> bool {
        // Tier 2: Glob re-exports, of this crate first, then of another
        // workspace crate.
        let holder = CrateModule::of(dep);
        let globs = |reach: GlobReach| bindings.globs().filter(move |glob| glob.reach == reach);
        let reexports = globs(GlobReach::ReExport);
        if let Some(glob) = self.glob_bringing(holder, reexports, symbol, meaning, chain) {
            tracing::debug!(
                "re-export resolved: {} -> {} (via glob re-export in {})",
                dep.target_module,
                glob.module,
                dep.target_module
            );
            dep.target_package.clone_from(&glob.crate_name);
            dep.target_module.clone_from(&glob.module);
            return true;
        }

        // Tier 2b: Private glob, visible only to descendants of the module
        // that holds it, like Tier 1b. One of this crate is passed through.
        let private = globs(GlobReach::Private);
        if let Some(glob) =
            self.glob_bringing(holder, private, symbol, meaning, &Visited::default())
        {
            if !glob.foreign {
                passed.push(PassedThrough::PrivateGlob {
                    holder: (dep.target_package.clone(), dep.target_module.clone()),
                    source: glob.module.clone(),
                });
            }
            dep.target_package.clone_from(&glob.crate_name);
            dep.target_module.clone_from(&glob.module);
            return true;
        }
        false
    }

    /// The module, by crate and path, that defines `name` by a macro, where
    /// `module` binds `name` by nothing the map lists and no glob brings it:
    /// of `module` and the modules its globs import from, as the viewer sees
    /// them, the only one whose items expand a macro. `None` where none or
    /// several do, where one of them holds a glob whose names the map does
    /// not know or binds `name` by an item no binding lists, and where a glob
    /// imports from a module the module tree does not hold.
    fn sole_macro_expander(self, module: CrateModule, name: &str) -> Option<(String, String)> {
        let mut expander = None;
        let walk = self.walk_globs(
            module,
            None,
            &mut Visited::default(),
            &mut |_, module, bindings| {
                let Some(bindings) = bindings else {
                    let known = self.module_paths.holds(module.crate_name, module.path);
                    return if known {
                        ControlFlow::Continue(())
                    } else {
                        ControlFlow::Break(())
                    };
                };
                let unknown_glob = bindings
                    .info
                    .unplaced_globs
                    .iter()
                    .any(|visibility| bindings.sees(visibility));
                if unknown_glob || bindings.info.unlisted.contains(name) {
                    return ControlFlow::Break(());
                }
                if bindings.info.expands_macros {
                    if expander.is_some() {
                        return ControlFlow::Break(());
                    }
                    expander = Some((module.crate_name.to_string(), module.path.to_string()));
                }
                ControlFlow::Continue(())
            },
        );
        if walk.is_break() {
            return None;
        }
        expander
    }

    /// The first of `globs`, globs held by `holder` that the viewer sees,
    /// whose source brings `symbol`, as `meaning` asks for it, through
    /// `holder` without entering a module in `passed`: a source that reaches
    /// it only through a module the chain already passed leads back, not on.
    fn glob_bringing<'g>(
        self,
        holder: CrateModule,
        mut globs: impl Iterator<Item = &'g GlobBinding>,
        symbol: &str,
        meaning: Meaning,
        passed: &Visited,
    ) -> Option<&'g GlobBinding> {
        globs.find(|glob| {
            self.through(holder)
                .brings_passing(glob.source_module(), symbol, meaning, passed.clone())
                .is_some()
        })
    }

    /// Whether `module` brings `symbol` into the scope of the module the
    /// viewer looks from, and in which builds: by its own definition, a `use`
    /// of any visibility, a name a `use` binds outside the crate, or a glob of
    /// any visibility, followed into other workspace crates, each only where
    /// the viewer sees it. Private `use` items that bind the symbol to
    /// different targets bring it in every build: the module holds it then in
    /// whichever, and a glob of the module does not go on to another. A `use`
    /// under a `#[cfg]` syn cannot decide brings it in some builds only,
    /// unless a glob of the module brings it in every one. `None` where the
    /// module does not bring the symbol.
    pub(crate) fn brings(self, module: CrateModule, symbol: &str) -> Option<Brought> {
        self.brings_passing(module, symbol, Meaning::Any, Visited::default())
    }

    /// [`NameLookup::brings`] for a reference of `meaning`, with the modules
    /// in `visited` bringing nothing.
    fn brings_passing(
        self,
        module: CrateModule,
        symbol: &str,
        meaning: Meaning,
        mut visited: Visited,
    ) -> Option<Brought> {
        let mut brought = None;
        let walk = self.walk_globs(module, None, &mut visited, &mut |_, _, bindings| {
            let Some(bindings) = bindings else {
                return ControlFlow::Continue(());
            };
            let named = bindings
                .named(symbol)
                .filter(|binding| binding.answers(meaning));
            if named.clone().any(|binding| binding.ends_the_chain(meaning)) {
                return ControlFlow::Break(Brought::InEveryBuild);
            }
            let binding = visible_binding(named.clone());
            let bound_in_every_build = binding
                .is_some_and(|binding| binding.builds == Brought::InEveryBuild)
                || binds_to_differing_targets(bindings, symbol)
                || binds_elsewhere(named);
            if bound_in_every_build {
                return ControlFlow::Break(Brought::InEveryBuild);
            }
            if binding.is_some() {
                brought = Some(Brought::InSomeBuilds);
            }
            ControlFlow::Continue(())
        });
        walk.break_value().or(brought)
    }

    /// Look up what `name` in `module` stands for, as the viewer sees it.
    /// `None` where the module binds nothing the viewer sees under the name.
    /// A child module the module tree holds stands for the name whether or
    /// not the viewer sees its declaration. The order is a child module, a `use` that binds a module, the module's
    /// own items and other `use` items, then its globs, so a binding of the
    /// module hides a glob as in rustc. Where the path goes on after the name,
    /// a `use` is followed through further `use` items to the module it
    /// binds. There a definition outside the type namespace stands for the
    /// name only where nothing else does, since a path goes on only through a
    /// module or a type.
    pub(crate) fn name(
        self,
        module: CrateModule,
        name: &str,
        after: AfterName,
    ) -> Option<NameTarget> {
        self.name_passing(module, name, after, &mut Visited::default())
    }

    /// The module `name` in `module` stands for, as [`NameLookup::name`] has
    /// it, without asking which item a glob brings under the name where no
    /// module stands for it.
    pub(crate) fn module_named(
        self,
        module: CrateModule,
        name: &str,
        after: AfterName,
    ) -> Option<NameTarget> {
        self.name_asking(module, name, after, &mut Visited::default(), Asked::Modules)
            .filter(|named| *named != NameTarget::Item)
    }

    /// [`NameLookup::name`], with the modules and names a chain of `use` items
    /// already passed in `visited`.
    pub(crate) fn name_passing(
        self,
        module: CrateModule,
        name: &str,
        after: AfterName,
        visited: &mut Visited,
    ) -> Option<NameTarget> {
        self.name_asking(module, name, after, visited, Asked::Everything)
    }

    /// [`NameLookup::name_passing`], asking the globs for an item only where
    /// `asked` is [`Asked::Everything`].
    fn name_asking(
        self,
        module: CrateModule,
        name: &str,
        after: AfterName,
        visited: &mut Visited,
        asked: Asked,
    ) -> Option<NameTarget> {
        let (lookup, bindings) = self.enter(module, Some(name), visited)?;
        let child = append_to_path(module.path, name);
        if self
            .module_paths
            .get_or_empty(module.crate_name)
            .contains(&child)
        {
            return Some(NameTarget::Module {
                crate_name: module.crate_name.to_string(),
                path: child,
            });
        }
        let bindings = bindings?;
        let named = bindings.named(name);
        for target in named.clone().filter_map(NameBinding::use_target) {
            if lookup.binds_a_module(target) {
                return Some(NameTarget::Module {
                    crate_name: target.crate_name.clone(),
                    path: append_to_path(&target.module, &target.original_name),
                });
            }
            if after == AfterName::PathGoesOn {
                let holder = CrateModule {
                    crate_name: &target.crate_name,
                    path: &target.module,
                };
                let bound =
                    lookup.name_asking(holder, &target.original_name, after, visited, asked);
                if let Some(module) = bound.filter(|bound| *bound != NameTarget::Item) {
                    return Some(module);
                }
            }
        }
        let item_here = named.clone().any(|binding| match binding.kind {
            BindingKind::Definition(_) => {
                after == AfterName::PathEnds || binding.namespace() == Some(Namespace::Type)
            }
            BindingKind::ReExport(_)
            | BindingKind::ReExportFromOutside(_)
            | BindingKind::PrivateUse(_)
            | BindingKind::Elsewhere => true,
            BindingKind::Module => false,
        });
        if item_here {
            return Some(NameTarget::Item);
        }
        let glob_module = bindings.globs().find_map(|glob| {
            lookup
                .through(module)
                .glob_child_module(glob.source_module(), name)
        });
        if let Some(module) = glob_module {
            return Some(module);
        }
        if asked == Asked::Modules {
            return None;
        }
        let brought = lookup
            .glob_bringing(
                module,
                bindings.globs(),
                name,
                Meaning::Any,
                &Visited::default(),
            )
            .is_some()
            || named
                .into_iter()
                .any(|binding| binding.definition().is_some());
        brought.then_some(NameTarget::Item)
    }

    /// The module `name` stands for in `module`, or in a module its globs
    /// import from: a child module whose declaration the viewer sees, or a
    /// module or crate root a `use` the viewer sees binds under the name. A
    /// private glob brings its names to the module's descendants too.
    fn glob_child_module(self, module: CrateModule, name: &str) -> Option<NameTarget> {
        let walk = self.walk_globs(
            module,
            None,
            &mut Visited::default(),
            &mut |lookup, module, bindings| {
                let bound = || {
                    let target = bindings?
                        .named(name)
                        .filter_map(NameBinding::use_target)
                        .find(|target| lookup.binds_a_module(target))?;
                    Some(NameTarget::Module {
                        crate_name: target.crate_name.clone(),
                        path: append_to_path(&target.module, &target.original_name),
                    })
                };
                match lookup.declared_child(module, bindings, name).or_else(bound) {
                    Some(module) => ControlFlow::Break(module),
                    None => ControlFlow::Continue(()),
                }
            },
        );
        walk.break_value()
    }

    /// The child module `name` of `module`, whose bindings the viewer sees as
    /// `bindings`, where the module tree holds it and the viewer sees its
    /// declaration. A module the map does not hold declares only public
    /// modules.
    pub(crate) fn declared_child(
        self,
        module: CrateModule,
        bindings: Option<SeenBindings>,
        name: &str,
    ) -> Option<NameTarget> {
        let child = append_to_path(module.path, name);
        let declared = self
            .module_paths
            .get_or_empty(module.crate_name)
            .contains(&child);
        let seen = bindings.is_none_or(|bindings| sees_the_module(bindings, name));
        (declared && seen).then(|| NameTarget::Module {
            crate_name: module.crate_name.to_string(),
            path: child,
        })
    }

    /// Whether the glob `dep` brings a trait into the module the viewer looks
    /// from, or a name whose kind the map does not know at its definer. A
    /// module the map does not hold counts as unknown, and so does a name the
    /// chain follows out of the map or a `use` binds outside the crate. The
    /// glob brings only the bindings the viewer sees, through the root `dep`
    /// resolves against.
    fn may_bring_a_trait(self, dep: &DependencyRef) -> bool {
        let reach = Some(GlobReach::ReExport);
        let walk = self.walk_globs(
            CrateModule::of(dep),
            reach,
            &mut Visited::default(),
            &mut |lookup, module, bindings| {
                let Some(bindings) = bindings else {
                    return ControlFlow::Break(());
                };
                let defines_a_trait = bindings.all().any(|(_, binding)| {
                    binding
                        .definition()
                        .is_some_and(|definition| definition.kind == DefKind::Trait)
                });
                let reexports_a_trait = bindings
                    .all()
                    .filter(|(_, binding)| binding.use_target().is_some())
                    .any(|(name, _)| {
                        let named = DependencyRef {
                            target_package: module.crate_name.to_string(),
                            target_module: module.path.to_string(),
                            target_item: Some(name.to_string()),
                            ..dep.clone()
                        };
                        lookup.reexports_a_trait_or_unknown(named)
                    });
                let binds_elsewhere = bindings
                    .all()
                    .any(|(_, binding)| binding.kind == BindingKind::Elsewhere);
                if defines_a_trait || reexports_a_trait || binds_elsewhere {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
        walk.is_break()
    }

    /// Whether the item `named` points at, re-exported by the module the
    /// viewer looks from, is a trait at its definer, or the chain ends on no
    /// definition the map knows. A name that ends on a module is no trait.
    /// Nothing in that module asks a meaning of the name, so the chain
    /// follows any.
    fn reexports_a_trait_or_unknown(self, mut named: DependencyRef) -> bool {
        if self.follow(&mut named, Meaning::Any).places_no_definer() {
            return true;
        }
        let Some(item) = &named.target_item else {
            return false;
        };
        self.viewer
            .bindings_in(CrateModule::of(&named))
            .and_then(|bindings| bindings.every(item).find_map(NameBinding::definition))
            .is_none_or(|definition| definition.kind == DefKind::Trait)
    }

    /// Point `dep` at the module `target` binds, when it binds a module or a
    /// crate root that the viewer sees: the reference then names the module,
    /// not an item in it. A reference whose `meaning` is no module goes on to
    /// an item of that name beside the module, where its holder shows one.
    fn end_on_module(
        self,
        dep: &mut DependencyRef,
        target: &ReExportTarget,
        meaning: Meaning,
    ) -> bool {
        let item_meant = || {
            let holder = CrateModule {
                crate_name: &target.crate_name,
                path: &target.module,
            };
            !meaning.admits_a_module() && self.holds_an_item_named(holder, &target.original_name)
        };
        let binds_module =
            self.binds_a_module(target) && self.module_is_visible(target) && !item_meant();
        if binds_module {
            dep.target_package.clone_from(&target.crate_name);
            dep.target_module = append_to_path(&target.module, &target.original_name);
            dep.target_item = None;
        }
        binds_module
    }

    /// Whether `module` holds an item `name` that the viewer sees: its own
    /// definition, or a `use` of something that is no module. Beside a module
    /// of that name, it is what a reference that does not mean the module
    /// names.
    pub(crate) fn holds_an_item_named(self, module: CrateModule, name: &str) -> bool {
        let Some(bindings) = self.viewer.bindings_in(module) else {
            return false;
        };
        let named = bindings.named(name);
        named.clone().any(|binding| binding.definition().is_some())
            || visible_binding(named)
                .and_then(NameBinding::use_target)
                .is_some_and(|target| !self.binds_a_module(target))
    }

    /// Where the item of the type namespace that `module` binds as `name` is
    /// defined, by a definition of its own or through `use` items, as the
    /// viewer sees it. `None` where `name` is bound to no such item; the
    /// modules and names in `visited` bind nothing.
    fn type_item_definer(
        self,
        module: CrateModule,
        name: &str,
        visited: &mut Visited,
    ) -> Option<ReExportTarget> {
        let (lookup, bindings) = self.enter(module, Some(name), visited)?;
        for binding in bindings?.named(name) {
            let definer = match &binding.kind {
                BindingKind::Definition(_) if binding.namespace() == Some(Namespace::Type) => Some(
                    target_in(module.crate_name.to_string(), module.path.to_string(), name),
                ),
                BindingKind::ReExport(target) | BindingKind::PrivateUse(target) => {
                    let holder = CrateModule {
                        crate_name: &target.crate_name,
                        path: &target.module,
                    };
                    lookup.type_item_definer(holder, &target.original_name, visited)
                }
                BindingKind::Definition(_)
                | BindingKind::ReExportFromOutside(_)
                | BindingKind::Elsewhere
                | BindingKind::Module => None,
            };
            if definer.is_some() {
                return definer;
            }
        }
        None
    }

    /// The namespace of the definition `module` binds as `name`, by a
    /// definition of its own or through `use` items, as the viewer sees it.
    /// `None` where no definition the map knows is reached; the modules and
    /// names in `visited` bind nothing.
    pub(crate) fn definition_namespace(
        self,
        module: CrateModule,
        name: &str,
        visited: &mut Visited,
    ) -> Option<Namespace> {
        let (lookup, bindings) = self.enter(module, Some(name), visited)?;
        for binding in bindings?.named(name) {
            let namespace = match &binding.kind {
                BindingKind::Definition(_) => binding.namespace(),
                BindingKind::ReExport(target) | BindingKind::PrivateUse(target) => {
                    let holder = CrateModule {
                        crate_name: &target.crate_name,
                        path: &target.module,
                    };
                    lookup.definition_namespace(holder, &target.original_name, visited)
                }
                BindingKind::ReExportFromOutside(_)
                | BindingKind::Elsewhere
                | BindingKind::Module => None,
            };
            if namespace.is_some() {
                return namespace;
            }
        }
        None
    }

    /// Whether the module of `target` defines its name, as the viewer sees
    /// it, and only outside the type namespace.
    fn defines_only_outside_the_type_namespace(self, target: &ReExportTarget) -> bool {
        let holder = CrateModule {
            crate_name: &target.crate_name,
            path: &target.module,
        };
        let Some(bindings) = self.viewer.bindings_in(holder) else {
            return false;
        };
        let mut named = bindings.every(&target.original_name).peekable();
        named.peek().is_some()
            && named.all(|binding| {
                binding.definition().is_some() && binding.namespace() != Some(Namespace::Type)
            })
    }

    /// Whether `target` binds a module of a workspace crate, or its root, as
    /// the module tree has it.
    pub(crate) fn binds_a_module(self, target: &ReExportTarget) -> bool {
        let module = append_to_path(&target.module, &target.original_name);
        if module.is_empty() {
            self.module_paths.contains_key(&target.crate_name)
        } else {
            self.module_paths
                .get_or_empty(&target.crate_name)
                .contains(&module)
        }
    }

    /// Whether the viewer sees the module `target` binds, as the `mod`
    /// declarations in its parent have it. A crate root is visible, and so is
    /// a module the map holds no restricted declaration of.
    pub(crate) fn module_is_visible(self, target: &ReExportTarget) -> bool {
        let parent = CrateModule {
            crate_name: &target.crate_name,
            path: &target.module,
        };
        self.viewer
            .bindings_in(parent)
            .is_none_or(|parent| sees_the_module(parent, &target.original_name))
    }
}

/// Whether a reference to `name` in the module of `bindings` names no definer
/// syn can place, and so produces no edge: private `use` items bind the name
/// to different targets and `binding` is one of them or missing, or, without
/// a `binding`, a `use` the viewer sees binds the name outside the crate. A
/// `binding` goes missing where its `use` is in builds the reference is not;
/// which of the other targets the reference has, the bindings do not tell.
fn places_no_definer(bindings: SeenBindings, name: &str, binding: Option<&NameBinding>) -> bool {
    match binding.map(|binding| &binding.kind) {
        Some(BindingKind::PrivateUse(_)) => private_targets_differ(bindings.every(name)),
        Some(BindingKind::ReExport(_)) => false,
        _ => binds_to_differing_targets(bindings, name) || binds_elsewhere(bindings.named(name)),
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

/// Whether the viewer of `bindings` sees a declaration of the child module
/// `name` among them, or there is none. A parent that declares only public
/// modules may be missing from the map.
pub(crate) fn sees_the_module(bindings: SeenBindings, name: &str) -> bool {
    let is_module = |binding: &NameBinding| binding.kind == BindingKind::Module;
    bindings.named(name).any(is_module) || !bindings.unseen(name).any(is_module)
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

/// The map as each build of a crate root sees it: the build without `test`
/// and, where the root has one, its test build. Which of the two reads a
/// piece of code follows from the code's context alone.
#[derive(Clone, Default)]
pub(crate) struct RootBuilds {
    production: ReExportMap,
    /// The map as the root's test build sees it, where the root has modules
    /// of its test build: the map of a file only test builds load.
    test_build: Option<ReExportMap>,
}

impl RootBuilds {
    /// The builds of the root `map` is seen from. A map seen from no root,
    /// or from one without modules of its test build, reads test code as
    /// built without `test`.
    pub(crate) fn of(map: ReExportMap) -> Self {
        Self {
            test_build: map.test_build(),
            production: map,
        }
    }

    /// The map that reads code of `kind`: test code in the root's test
    /// build, where it has one, production code in the build without `test`.
    pub(crate) fn reading(&self, kind: UsageKind) -> &ReExportMap {
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
    pub(crate) fn definitions(
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
pub(crate) struct CrateView<'a> {
    /// The crate's key in `module_paths`: a package key, or for an
    /// integration test the full path of its root module.
    pub(crate) crate_name: &'a str,
    pub(crate) workspace_crates: &'a WorkspacePackages,
    pub(crate) module_paths: &'a ModulePathMap,
    pub(crate) crate_exports: &'a PackageExportMap,
    /// Code-side crate name -> `package_id` for external crates visible to
    /// the crate. Populated from `crate_name_map[crate_name]`.
    pub(crate) external_crate_names: &'a HashMap<String, String>,
    /// The workspace crates the extern prelude of the root whose files are
    /// read names, in every module of the crate.
    pub(crate) extern_prelude: &'a ExternPrelude,
    pub(crate) builds: RootBuilds,
    pub(crate) modules_bound: ModulesBound,
}

/// The module each module of a crate binds under a name, as
/// `PathEnd::of_path` asks it, kept once it is looked up. Every path of a
/// module that starts with the same name asks the same question, and the
/// answer walks the module's globs.
#[derive(Default)]
pub(crate) struct ModulesBound(pub(crate) RefCell<HashMap<BoundQuestion, Option<NameTarget>>>);

/// What [`ModulesBound`] is asked: whether the build is the test build, the
/// module, the name, and whether the path goes on after it.
type BoundQuestion = (bool, String, String, AfterName);

/// Invariant context for dependency resolution within a single source file,
/// in one build of its root.
pub(crate) struct ResolutionContext<'a> {
    pub(crate) crate_view: &'a CrateView<'a>,
    pub(crate) source_file: &'a Path,
    pub(crate) current_module_path: &'a str,
    reexport_map: &'a ReExportMap,
    /// Whether the code read is the root's test build.
    pub(crate) test_build: bool,
}

impl<'a> ResolutionContext<'a> {
    /// The context of `source_file`, the file of `current_module_path` in
    /// the crate of `crate_view`, reading production code.
    pub(crate) fn new(
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
    pub(crate) fn in_module<'m>(&'m self, module: &'m str) -> ResolutionContext<'m> {
        ResolutionContext {
            current_module_path: module,
            ..*self
        }
    }

    /// The same file, reading code of `kind`.
    pub(crate) fn reading(&self, kind: UsageKind) -> Self {
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
    pub(crate) fn viewer_for(&self, dep: &DependencyRef) -> Viewer<'a> {
        self.viewer(dep.target_root)
    }

    /// The name lookup from this file's module, looking through `root`.
    pub(crate) fn lookup(&self, root: TargetRoot) -> NameLookup<'a> {
        NameLookup {
            viewer: self.viewer(root),
            module_paths: self.crate_view.module_paths,
        }
    }

    /// The name lookup from this file's module that reads what `dep` points
    /// at.
    pub(crate) fn lookup_for(&self, dep: &DependencyRef) -> NameLookup<'a> {
        self.lookup(dep.target_root)
    }

    /// Whether the module of this file has a child module `name` in the
    /// module tree.
    pub(crate) fn has_child_module(&self, name: &str) -> bool {
        self.crate_view
            .module_paths
            .get_or_empty(&normalize_crate_name(self.crate_view.crate_name))
            .contains(&append_to_path(self.current_module_path, name))
    }

    /// The bindings of the module `dep` points at, as this file sees it.
    pub(crate) fn target_bindings(&self, dep: &DependencyRef) -> Option<SeenBindings<'a>> {
        self.viewer_for(dep).bindings_in(CrateModule::of(dep))
    }

    /// [`NameLookup::follow`] from this file's module, through the root `dep`
    /// resolves against, for a reference that asks `meaning` of `dep`'s item.
    pub(crate) fn resolve_reexport(&self, dep: &mut DependencyRef, meaning: Meaning) -> Resolution {
        self.lookup_for(dep).follow(dep, meaning)
    }

    /// [`ResolutionContext::resolve_reexport`], with what the chain passed
    /// through noted in `passed_on`. Whether the chain places a definer, so
    /// that `dep` produces an edge.
    pub(crate) fn resolve_noting(
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
    pub(crate) fn admits_a_module(self) -> bool {
        matches!(self, Meaning::Type | Meaning::Any)
    }

    /// Whether a binding in `namespace` is what this meaning may name.
    pub(crate) fn admits(self, namespace: Namespace) -> bool {
        match self {
            Meaning::Type => namespace == Namespace::Type,
            Meaning::Value => namespace == Namespace::Value,
            Meaning::Macro => namespace == Namespace::Macro,
            Meaning::Any => true,
        }
    }
}

/// Join a prefix and segment with `::`, handling empty prefix.
pub(crate) fn append_to_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
    }
}

/// `path` followed by `rest`, `path` alone where `rest` is empty.
pub(crate) fn followed_by(path: &str, rest: &str) -> String {
    if rest.is_empty() {
        path.to_string()
    } else {
        format!("{path}::{rest}")
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

/// Resolve `super::` and `self::` relative paths to absolute crate-local paths.
///
/// Returns `None` when the path is not relative, or when too many `super::`
/// would go above crate root.
pub(crate) fn resolve_relative_path(path: &str, current_module_path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split("::").collect();
    let super_count = segments.iter().take_while(|&&s| s == "super").count();

    if super_count > 0 {
        return join_module_segments(current_module_path, super_count, &segments[super_count..]);
    }

    if segments.first() == Some(&"self") {
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

/// The segments of the module path `path`, none for the crate root (`""`).
pub(crate) fn module_segments(path: &str) -> impl Iterator<Item = &str> {
    path.split("::").filter(|segment| !segment.is_empty())
}

/// What the first name of a path stands for in the module the path is
/// written in, as far as the one who asks can tell.
pub(crate) enum FirstName {
    /// A module: a child module, or one a binding of the module names.
    Module { crate_name: String, path: String },
    /// An item of the type namespace the module defines.
    TypeItem,
    /// Something only the binding table tells, asked while it is collected.
    Unknown,
    /// Nothing the module binds.
    Unbound,
}

/// Where a path starts, picked in rustc's order: `self::` and `super::` an
/// ancestor of the module, `crate::` the root, then what the first name
/// stands for in the module, then a crate of the extern prelude.
pub(crate) enum PickedStart<'p> {
    /// The own crate, with the path from its root, `::`-joined.
    OwnRoot(String),
    /// A module the first name stands for, with the rest of the path, `""`
    /// where it ends on the name.
    Module {
        crate_name: String,
        path: String,
        rest: &'p str,
    },
    /// The item `name` of the module; the rest of the path names something
    /// in the item.
    TypeItem { name: &'p str },
    /// A name only the binding table tells, then the rest of the path.
    Unknown { rest: &'p str },
    /// The workspace crate the extern prelude names, with the rest.
    Crate { crate_name: String, rest: &'p str },
    /// None of these: an external crate, or nothing.
    Outside,
}

impl<'p> PickedStart<'p> {
    /// The start of `path`, written in `module_path`. `first_name` tells what
    /// the first name stands for in the module, and is asked only where the
    /// path starts with a name. An unknown first name gives way to a crate
    /// of `extern_prelude`: in a `use` path a name both bound and a crate is
    /// ambiguous to rustc, so the crate is what compiles. `None` where the
    /// path starts with an empty segment.
    pub(crate) fn of(
        path: &'p str,
        module_path: &str,
        extern_prelude: &HashMap<String, String>,
        first_name: impl FnOnce(&str, AfterName) -> FirstName,
    ) -> Option<Self> {
        if let Some(resolved) = resolve_relative_path(path, module_path) {
            return Some(Self::OwnRoot(resolved));
        }
        if let Some(after_crate) = path.strip_prefix("crate::") {
            return Some(Self::OwnRoot(after_crate.to_string()));
        }
        let (first, rest) = path.split_once("::").unwrap_or((path, ""));
        if first.is_empty() {
            return None;
        }
        let in_prelude = || {
            extern_prelude.get(first).map(|crate_name| Self::Crate {
                crate_name: crate_name.clone(),
                rest,
            })
        };
        let after = if rest.is_empty() {
            AfterName::PathEnds
        } else {
            AfterName::PathGoesOn
        };
        Some(match first_name(first, after) {
            FirstName::Module { crate_name, path } => Self::Module {
                crate_name,
                path,
                rest,
            },
            FirstName::TypeItem => Self::TypeItem { name: first },
            FirstName::Unknown => in_prelude().unwrap_or(Self::Unknown { rest }),
            FirstName::Unbound => in_prelude().unwrap_or(Self::Outside),
        })
    }
}

/// The workspace crates in the extern prelude of one crate root: name in
/// the code -> key of the crate named, in the root's build without `test`
/// and in its test build. Only these names start a path at a workspace
/// crate.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExternPrelude {
    without_test: HashMap<String, String>,
    test: HashMap<String, String>,
}

impl ExternPrelude {
    pub(crate) fn new(
        without_test: HashMap<String, String>,
        test: HashMap<String, String>,
    ) -> Self {
        Self { without_test, test }
    }

    /// The names of the test build where `test` is set, otherwise of the
    /// build without `test`.
    pub(crate) fn names(&self, test: bool) -> &HashMap<String, String> {
        if test { &self.test } else { &self.without_test }
    }
}

/// The module of a workspace crate a `use` is written in, with what places
/// its paths while the binding table is collected: the module tree of every
/// workspace crate and the names that stand for one.
pub(crate) struct UseScope<'a> {
    /// The crate of the module, normalized.
    pub(crate) crate_name: &'a str,
    pub(crate) module_path: &'a str,
    pub(crate) module_paths: &'a ModulePathMap,
    /// The workspace crates of the extern prelude, by name, in the build the
    /// `use` is read in ([`ExternPrelude::names`]).
    pub(crate) extern_prelude: &'a HashMap<String, String>,
    /// The names of the type items the module defines.
    pub(crate) type_items: &'a HashSet<String>,
}

/// What a `use` path binds, placed by the module tree alone.
#[derive(Debug)]
pub(crate) enum UsePlace {
    /// A name in a module of a workspace crate, or a crate root as the name
    /// `""` in its own root. Where the path starts from a name in scope, its
    /// first name is a binding of the module or an external crate
    /// (`from_scope`), and the target holds the path as written, from the
    /// root.
    Name {
        target: ReExportTarget,
        from_scope: bool,
    },
    /// The names a glob takes from a module, or the path of the glob as
    /// written where it goes on past the modules of the tree.
    Glob { crate_name: String, module: String },
    /// Nothing: the path names its own crate alone.
    Nothing,
}

impl UseScope<'_> {
    /// Place the `use` path `path` by the module tree. Which names the
    /// modules bind is not asked, since the table that holds them is what
    /// is being collected. From the start [`UseScope::start`] picks, each
    /// segment that names a child module enters it, and the next segment is
    /// the name. A path that ends on a module binds it as a name in its
    /// parent; a whole workspace crate binds its root. A path that goes on
    /// past the name keeps the path as written, with its last segment as the
    /// name, for [`ReExportMap::settle_written_paths`] to follow once the
    /// table is whole, in this crate as in another. A glob takes the names
    /// of the module its path ends on; one whose path goes on past the
    /// modules of the tree keeps it as written the same way. `None` where no
    /// start fits.
    pub(crate) fn place(&self, path: &str) -> Option<UsePlace> {
        let (crate_name, segments, from_scope) = self.start(path)?;
        if segments.first().is_some_and(String::is_empty) {
            return None;
        }
        let foreign = crate_name != self.crate_name;
        if let Some((last, base)) = segments.split_last()
            && last == "*"
        {
            let (module, rest) = self.enter_modules(&crate_name, base);
            let module = if rest.is_empty() {
                module
            } else {
                base.join("::")
            };
            return Some(UsePlace::Glob { crate_name, module });
        }
        let (module, rest) = self.enter_modules(&crate_name, &segments);
        let (module, name) = match rest {
            [] if module.is_empty() && !foreign => return Some(UsePlace::Nothing),
            [] => {
                let (parent, name) = module.rsplit_once("::").unwrap_or(("", module.as_str()));
                (parent.to_string(), name.to_string())
            }
            [name] => (module, name.clone()),
            [.., last] => {
                let written = &segments[..segments.len() - 1];
                (written.join("::"), last.clone())
            }
        };
        let target = ReExportTarget {
            crate_name,
            module,
            original_name: name,
        };
        Some(UsePlace::Name { target, from_scope })
    }

    /// The crate a `use` path starts in, its segments from that crate's root,
    /// and whether it starts from a name in scope. [`PickedStart::of`] picks
    /// the start, with what this module binds as far as the table being
    /// collected tells: its child modules and the type items it defines. Any
    /// other first name the path goes on past is a binding of this module or
    /// an external crate: the path starts in this module and is marked as
    /// from scope, and [`ReExportMap::settle_written_paths`] tells which once
    /// the table is whole.
    fn start(&self, path: &str) -> Option<(String, Vec<String>, bool)> {
        let own = |in_crate: &str, from_scope: bool| {
            let segments = in_crate.split("::").map(ToString::to_string);
            Some((self.crate_name.to_string(), segments.collect(), from_scope))
        };
        let first_name = |first: &str, _| {
            let child = append_to_path(self.module_path, first);
            if self.module_paths.holds(self.crate_name, &child) {
                FirstName::Module {
                    crate_name: self.crate_name.to_string(),
                    path: child,
                }
            } else if self.type_items.contains(first) {
                FirstName::TypeItem
            } else {
                FirstName::Unknown
            }
        };
        match PickedStart::of(path, self.module_path, self.extern_prelude, first_name)? {
            PickedStart::OwnRoot(from_root) => own(&from_root, false),
            PickedStart::Module { path, rest, .. } => own(&followed_by(&path, rest), false),
            PickedStart::TypeItem { .. } => own(&append_to_path(self.module_path, path), false),
            PickedStart::Crate { crate_name, rest } => {
                let segments = module_segments(rest).map(ToString::to_string).collect();
                Some((crate_name, segments, false))
            }
            PickedStart::Unknown { rest } if !rest.is_empty() => {
                own(&append_to_path(self.module_path, path), true)
            }
            PickedStart::Unknown { .. } | PickedStart::Outside => None,
        }
    }

    /// Enter each of `segments` that names a child module, from the root of
    /// `crate_name`: the module reached and the segments after it.
    fn enter_modules<'s>(
        &self,
        crate_name: &str,
        segments: &'s [String],
    ) -> (String, &'s [String]) {
        let module_paths = self.module_paths.get_or_empty(crate_name);
        let mut module = String::new();
        let mut rest = segments;
        while let Some((first, tail)) = rest.split_first() {
            let child = append_to_path(&module, first);
            if !module_paths.contains(&child) {
                break;
            }
            module = child;
            rest = tail;
        }
        (module, rest)
    }
}

/// The `*` marker on a glob's dependency. It must survive upstream of here:
/// `collect_use_reexports` reads it out of its own `resolve_use_tree` pass to
/// hold the module's globs, which is what `resolve_reexport` resolves glob
/// chains against.
pub(crate) fn is_glob(dep: &DependencyRef) -> bool {
    dep.target_item.as_deref() == Some("*")
}

#[cfg(test)]
mod tests;
