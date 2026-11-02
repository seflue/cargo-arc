//! Syn-based use statement parsing for workspace dependency extraction.

use super::packages::ModulePathMap;
use crate::model::{DefKind, Definition, DependencyRef, EntryKind, TargetRoot};
use std::collections::{HashMap, HashSet};
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
    pub(crate) fn ends_the_chain(&self, meaning: Meaning) -> bool {
        self.answers(meaning)
            && matches!(
                self.kind,
                BindingKind::Definition(_) | BindingKind::ReExportFromOutside(_)
            )
    }

    /// Whether the binding may be what a reference of `meaning` names: a
    /// macro answers only a reference that may name one.
    pub(crate) fn answers(&self, meaning: Meaning) -> bool {
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
    pub(crate) globs: Vec<GlobBinding>,
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
    pub(crate) fn source_module(&self) -> CrateModule<'_> {
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
    pub(crate) info: &'a ModuleExportInfo,
    viewer: Viewer<'a>,
}

impl<'a> SeenBindings<'a> {
    pub(crate) fn sees(self, visibility: &Visibility) -> bool {
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
    pub(crate) fn unseen(
        self,
        name: &str,
    ) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
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
    pub(crate) fn globs(self) -> impl Iterator<Item = &'a GlobBinding> {
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
    pub(crate) packages: Arc<HashMap<String, Vec<RootReExports>>>,
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
    pub(crate) fn viewer<'a>(&'a self, root: TargetRoot, module: &'a str) -> Viewer<'a> {
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
    pub(crate) fn viewer_for<'a>(&'a self, dep: &DependencyRef, module: &'a str) -> Viewer<'a> {
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

    /// Each module of the map, in every root and build.
    pub(crate) fn module_infos(&self) -> impl Iterator<Item = (ModuleKey, &ModuleExportInfo)> {
        self.packages.iter().flat_map(|(package, roots)| {
            roots.iter().enumerate().flat_map(move |(root, exports)| {
                exports.builds().flat_map(move |(test_build, modules)| {
                    modules.iter().map(move |(path, info)| {
                        let module = ModuleKey {
                            root: (package.clone(), root),
                            test_build,
                            path: path.clone(),
                        };
                        (module, info)
                    })
                })
            })
        })
    }

    /// Each `use` binding of the map, in every root and build, with its place.
    pub(crate) fn use_targets(&self) -> impl Iterator<Item = (BindingPlace, &ReExportTarget)> {
        self.module_infos().flat_map(|(module, info)| {
            info.use_targets()
                .map(move |(name, index, re_export, target)| {
                    let place = BindingPlace {
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

    /// The export info of `module`.
    fn module_mut(&mut self, module: &ModuleKey) -> Option<&mut ModuleExportInfo> {
        let (package, root) = &module.root;
        let exports = Arc::make_mut(&mut self.packages)
            .get_mut(package)?
            .get_mut(*root)?;
        let modules = if module.test_build {
            exports.test_build_modules.as_mut()?
        } else {
            &mut exports.modules
        };
        modules.get_mut(&module.path)
    }

    /// Settle the `use` binding at each place: point it at its target, or,
    /// where it names something outside the workspace, bind a private `use`
    /// [`BindingKind::Elsewhere`] and a `pub use`
    /// [`BindingKind::ReExportFromOutside`].
    pub(crate) fn rebind(&mut self, settled: Vec<(BindingPlace, Settled)>) {
        let mut outside: HashMap<(ModuleKey, String), HashMap<usize, BindingKind>> = HashMap::new();
        for (place, settled) in settled {
            let kind = match settled {
                Settled::Target(target) => {
                    let binding = self
                        .module_mut(&place.module)
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
            outside
                .entry((place.module, place.name))
                .or_default()
                .insert(place.index, kind);
        }
        for ((module, name), mut kinds) in outside {
            let Some(info) = self.module_mut(&module) else {
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
    pub(crate) fn reglob(&mut self, settled: Vec<(GlobPlace, NameTarget)>) {
        let mut by_module: HashMap<ModuleKey, HashMap<usize, NameTarget>> = HashMap::new();
        for (place, reached) in settled {
            by_module
                .entry(place.module)
                .or_default()
                .insert(place.index, reached);
        }
        for (module, reached) in by_module {
            let Some(info) = self.module_mut(&module) else {
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
                        glob.foreign = *crate_name != module.root.0;
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

/// Where a binding sits in a [`ReExportMap`]: module, name, and place among
/// the bindings of the name.
pub(crate) struct BindingPlace {
    pub(crate) module: ModuleKey,
    name: String,
    index: usize,
    /// Whether the binding is a `pub use`.
    pub(crate) re_export: bool,
    /// Whether the `use` path of the binding starts from a name in scope.
    pub(crate) from_scope: bool,
}

/// A module of a [`ReExportMap`] by package, root, build and path.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct ModuleKey {
    /// The package and the index of its root, as [`ReExportMap`] holds its
    /// own root.
    pub(crate) root: (String, usize),
    pub(crate) test_build: bool,
    pub(crate) path: String,
}

/// Where a glob sits in a [`ReExportMap`]: module, and place among the globs
/// of the module.
pub(crate) struct GlobPlace {
    pub(crate) module: ModuleKey,
    pub(crate) index: usize,
}

/// What a written `use` path settles to: the target it reaches, or, where a
/// path from a name in scope reaches nothing in the workspace, what its
/// binding becomes.
pub(crate) enum Settled {
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
    pub(crate) packages: &'a HashMap<String, Vec<RootReExports>>,
    /// The root seen from inside, by package and entry-file index: its
    /// restricted bindings are visible below their module. Every other root
    /// shows only what is public, and every other package its lib root.
    pub(crate) own_root: Option<&'a (String, usize)>,
    /// Whether the own root is read as its test build loads it.
    pub(crate) test_build: bool,
    /// The module of the own root whose restricted bindings the viewer sees
    /// from, or `None` where it sees none of them.
    pub(crate) module: Option<&'a str>,
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
    pub(crate) fn through(self, holder: CrateModule) -> Self {
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
    pub(crate) fn reaching(self, crate_name: &str) -> Self {
        Self {
            test_build: self.test_build && self.own_root.is_none_or(|(own, _)| own == crate_name),
            ..self
        }
    }
}

/// A module holding a private `use` or glob, by crate and module path.
pub(crate) type Holder = (String, String);

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
pub(crate) type DefinedAt = (String, String, String);

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

#[cfg(test)]
mod tests;
