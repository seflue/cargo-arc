//! Syn-based use statement parsing for workspace dependency extraction.

use super::packages::{ModulePathMap, PackageExportMap, WorkspacePackages, normalize_crate_name};
use crate::model::{
    DefKind, Definition, DependencyRef, EdgeContext, EntryKind, SymbolUse, TargetRoot, TestKind,
    UsageKind, UseCategory,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::{ControlFlow, Deref};
use std::path::Path;
use std::sync::Arc;
use syn::Token;
use syn::UseTree;
use syn::punctuated::Punctuated;
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
    fn namespace(&self) -> Option<Namespace> {
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

    /// The target of each `use` binding, with its name and its place among
    /// the bindings of the name.
    fn use_targets(&self) -> impl Iterator<Item = (&str, usize, &ReExportTarget)> {
        self.bindings.iter().flat_map(|(name, bindings)| {
            bindings
                .iter()
                .enumerate()
                .filter_map(move |(index, binding)| {
                    let target = binding.use_target()?;
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

/// The bindings of one module as a [`Viewer`] sees them, the answer to "name
/// N in module M, seen by V". Each lookup gives what the viewer sees; what it
/// does not see comes only from [`SeenBindings::unseen`] and
/// [`SeenBindings::every`].
#[derive(Debug, Clone, Copy)]
struct SeenBindings<'a> {
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
    fn named(self, name: &str) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
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
    fn every(self, name: &str) -> impl Iterator<Item = &'a NameBinding> + Clone + use<'a> {
        self.info.named(name)
    }

    /// Every binding of every name the viewer sees, in the order of
    /// [`ModuleExportInfo::all`].
    fn all(self) -> impl Iterator<Item = (&'a str, &'a NameBinding)> {
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
    fn associated(self, name: &str) -> Option<&'a AssociatedItems> {
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

    /// What each written `use` target reaches, where its path waits on none
    /// of `unsettled`. A path not from a name in scope whose first name the
    /// root binds nothing under stays written.
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
                    Reached::Unbound if place.from_scope => Settled::Outside,
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
                info.use_targets().map(move |(name, index, target)| {
                    let place = BindingPlace {
                        package: package.clone(),
                        root,
                        test_build,
                        module: module.clone(),
                        name: name.to_string(),
                        index,
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
        let mut outside: HashMap<(String, usize, bool, String, String), HashSet<usize>> =
            HashMap::new();
        for (place, settled) in settled {
            let target = match settled {
                Settled::Target(target) => target,
                Settled::Outside => {
                    let key = (
                        place.package,
                        place.root,
                        place.test_build,
                        place.module,
                        place.name,
                    );
                    outside.entry(key).or_default().insert(place.index);
                    continue;
                }
            };
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
        }
        for ((package, root, test_build, module, name), indices) in outside {
            let Some(info) = self.module_mut(&package, root, test_build, &module) else {
                continue;
            };
            let Some(bindings) = info.bindings.remove(&name) else {
                continue;
            };
            // Bound again oldest first, so the order of the kinds holds.
            for (index, mut binding) in bindings.into_iter().enumerate().rev() {
                if indices.contains(&index) {
                    binding.kind = match binding.kind {
                        BindingKind::ReExport(written) => {
                            // Written from a name in scope, the path starts
                            // with the module that holds it.
                            let path = append_to_path(&written.module, &written.original_name);
                            let from_holder = path
                                .strip_prefix(&format!("{module}::"))
                                .map_or(path.clone(), ToString::to_string);
                            BindingKind::ReExportFromOutside(from_holder)
                        }
                        _ => BindingKind::Elsewhere,
                    };
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

/// What a written `use` path settles to: a target, or something outside the
/// workspace.
enum Settled {
    Target(ReExportTarget),
    Outside,
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

/// The modules a walk of a [`NameLookup`] already entered, by crate and module
/// path, with the name asked there where one walk asks several names.
#[derive(Debug, Default, Clone)]
struct Visited(HashSet<(String, String, String)>);

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
enum AfterName {
    /// The name is the last segment of the path.
    PathEnds,
    /// More segments follow, which the name has to carry on.
    PathGoesOn,
}

impl AfterName {
    /// What the path does after a name `rest` follows.
    fn followed_by(rest: &[&str]) -> Self {
        if rest.is_empty() {
            Self::PathEnds
        } else {
            Self::PathGoesOn
        }
    }
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

/// The lookup of "what is name N in module M for viewer V": a viewer and the
/// module tree. Each question walks the binding table through
/// [`NameLookup::enter`], which reads a module with the viewer that reaches
/// its crate, records it in the walk's [`Visited`] and gives only the
/// bindings the viewer sees. No question calls [`Viewer::reaching`] or
/// records a module itself.
#[derive(Debug, Clone, Copy)]
struct NameLookup<'a> {
    viewer: Viewer<'a>,
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
    fn brings(self, module: CrateModule, symbol: &str) -> Option<Brought> {
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
    fn name(self, module: CrateModule, name: &str, after: AfterName) -> Option<NameTarget> {
        self.name_passing(module, name, after, &mut Visited::default())
    }

    /// The module `name` in `module` stands for, as [`NameLookup::name`] has
    /// it, without asking which item a glob brings under the name where no
    /// module stands for it.
    fn module_named(self, module: CrateModule, name: &str, after: AfterName) -> Option<NameTarget> {
        self.name_asking(module, name, after, &mut Visited::default(), Asked::Modules)
            .filter(|named| *named != NameTarget::Item)
    }

    /// [`NameLookup::name`], with the modules and names a chain of `use` items
    /// already passed in `visited`.
    fn name_passing(
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
    fn declared_child(
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
    fn holds_an_item_named(self, module: CrateModule, name: &str) -> bool {
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
    fn definition_namespace(
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
    fn binds_a_module(self, target: &ReExportTarget) -> bool {
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
    fn module_is_visible(self, target: &ReExportTarget) -> bool {
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
fn sees_the_module(bindings: SeenBindings, name: &str) -> bool {
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
/// [`PathEnd::of_path`] asks it, kept once it is looked up. Every path of a
/// module that starts with the same name asks the same question, and the
/// answer walks the module's globs.
#[derive(Default)]
pub(crate) struct ModulesBound(RefCell<HashMap<BoundQuestion, Option<NameTarget>>>);

/// What [`ModulesBound`] is asked: whether the build is the test build, the
/// module, the name, and whether the path goes on after it.
type BoundQuestion = (bool, String, String, AfterName);

/// Invariant context for dependency resolution within a single source file,
/// in one build of its root.
pub(crate) struct ResolutionContext<'a> {
    crate_view: &'a CrateView<'a>,
    source_file: &'a Path,
    current_module_path: &'a str,
    reexport_map: &'a ReExportMap,
    /// Whether the code read is the root's test build.
    test_build: bool,
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
    fn in_module<'m>(&'m self, module: &'m str) -> ResolutionContext<'m> {
        ResolutionContext {
            current_module_path: module,
            ..*self
        }
    }

    /// The same file, reading code of `kind`.
    fn reading(&self, kind: UsageKind) -> Self {
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
    fn viewer_for(&self, dep: &DependencyRef) -> Viewer<'a> {
        self.viewer(dep.target_root)
    }

    /// The name lookup from this file's module, looking through `root`.
    fn lookup(&self, root: TargetRoot) -> NameLookup<'a> {
        NameLookup {
            viewer: self.viewer(root),
            module_paths: self.crate_view.module_paths,
        }
    }

    /// The name lookup from this file's module that reads what `dep` points
    /// at.
    fn lookup_for(&self, dep: &DependencyRef) -> NameLookup<'a> {
        self.lookup(dep.target_root)
    }

    /// Whether the module of this file has a child module `name` in the
    /// module tree.
    fn has_child_module(&self, name: &str) -> bool {
        self.crate_view
            .module_paths
            .get_or_empty(&normalize_crate_name(self.crate_view.crate_name))
            .contains(&append_to_path(self.current_module_path, name))
    }

    /// The bindings of the module `dep` points at, as this file sees it.
    fn target_bindings(&self, dep: &DependencyRef) -> Option<SeenBindings<'a>> {
        self.viewer_for(dep).bindings_in(CrateModule::of(dep))
    }

    /// [`NameLookup::follow`] from this file's module, through the root `dep`
    /// resolves against, for a reference that asks `meaning` of `dep`'s item.
    fn resolve_reexport(&self, dep: &mut DependencyRef, meaning: Meaning) -> Resolution {
        self.lookup_for(dep).follow(dep, meaning)
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

/// Where a `use` binds: an inline module body, a block, or the file itself.
pub(crate) type RegionId = usize;

/// The binding regions of one file, as parent links.
///
/// A `use` is an item and binds in its enclosing module or block only. A later
/// reference therefore sees a binding when the binding's region is the
/// reference's own region or one enclosing it. A block sees the region
/// around it. An inline module sees the module around it only where it
/// writes `use super::*`, which brings every name in scope there: the file or
/// the next inline module out, past the blocks in between. An item a region
/// declares hides every name of the same spelling from around it.
///
/// Both collectors number the regions of the same file, and a reference from one
/// walk is looked up against bindings from the other. The two numberings have to
/// agree, which is why both take them from `impl_binding_region_visits!` and
/// neither prunes its traversal.
#[derive(Debug, Clone)]
pub(crate) struct BindingRegions {
    parents: Vec<Option<RegionId>>,
    /// The regions of inline modules that do not see the region around them.
    closed: HashSet<RegionId>,
    /// For each region, the module it belongs to: itself for the file and an
    /// inline module, the module around it for a block.
    modules: Vec<RegionId>,
    /// The names of the items each region declares. Only the `use` collector
    /// records them.
    declared: HashMap<RegionId, HashSet<String>>,
}

impl BindingRegions {
    /// The file itself. Encloses every other region and is enclosed by none.
    pub(crate) const ROOT: RegionId = 0;

    fn open(&mut self, parent: RegionId) -> RegionId {
        self.parents.push(Some(parent));
        self.modules.push(self.modules[parent]);
        self.parents.len() - 1
    }

    /// A region an inline module opens, closed to the region around it
    /// until [`BindingRegions::see_around`] opens it.
    fn open_module(&mut self, parent: RegionId) -> RegionId {
        let region = self.open(parent);
        self.modules[region] = region;
        self.closed.insert(region);
        region
    }

    /// Let the inline module of `region` see the module around it, as its
    /// `use super::*` does. A block sees the region around it already.
    fn see_around(&mut self, region: RegionId) {
        self.closed.remove(&region);
        if let Some(parent) = self.parents[region] {
            self.parents[region] = Some(self.modules[parent]);
        }
    }

    fn declare(&mut self, region: RegionId, name: String) {
        self.declared.entry(region).or_default().insert(name);
    }

    /// Whether `region` declares an item named `name`.
    fn declares(&self, region: RegionId, name: &str) -> bool {
        self.declared
            .get(&region)
            .is_some_and(|names| names.contains(name))
    }

    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.parents.len()
    }

    /// A region and everything enclosing it, innermost first.
    fn enclosing(&self, region: RegionId) -> impl Iterator<Item = RegionId> + '_ {
        std::iter::successors(Some(region), move |&inner| {
            if self.closed.contains(&inner) {
                return None;
            }
            self.parents.get(inner).copied().flatten()
        })
    }
}

impl Default for BindingRegions {
    fn default() -> Self {
        Self {
            parents: vec![None],
            closed: HashSet::new(),
            modules: vec![Self::ROOT],
            declared: HashMap::new(),
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
        outer.promoted_to_test()
    } else {
        outer.clone()
    }
}

/// Shared `visit_item_mod`, `visit_item_fn` and `visit_block` for both collectors.
///
/// `visit_item_mod` and `visit_block` open a binding region. `visit_item_mod`
/// also carries the path of the inline module, which counts modules only:
/// `use super::X` in a function body still means the parent module.
/// `visit_item_fn` carries the test context of a fn with `#[test]`; each
/// collector's `visit_item` carries the one of any item with `#[cfg(test)]`.
macro_rules! impl_binding_region_visits {
    () => {
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            let prev_inline_path = Arc::clone(&self.inline_path);
            let prev_region = self.region;
            // Inline modules (with body) extend the path; `mod foo;` (external) does not
            if node.content.is_some() {
                self.inline_path =
                    append_to_path(&prev_inline_path, &node.ident.to_string()).into();
                self.region = self.regions.open_module(prev_region);
            }
            syn::visit::visit_item_mod(self, node);
            self.inline_path = prev_inline_path;
            self.region = prev_region;
        }

        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            let prev_context = self.context.clone();
            if is_test_fn(&node.attrs) {
                self.context = self.context.promoted_to_test();
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
    /// The inline modules around the item, as a path below the file's
    /// module: `a::b` inside `mod a { mod b { … } }`, empty outside any.
    pub(crate) inline_path: Arc<str>,
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
        inline_path: Arc<str>,
        regions: BindingRegions,
        region: RegionId,
    }
    impl<'ast> Visit<'ast> for UseCollector {
        fn visit_item(&mut self, node: &'ast syn::Item) {
            let prev_context = self.context.clone();
            self.context = item_context(node, &prev_context);
            if let Some(ident) = item_ident(node) {
                self.regions.declare(self.region, ident.to_string());
            }
            syn::visit::visit_item(self, node);
            self.context = prev_context;
        }

        fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
            if !attributes_expand(&node.attrs) {
                syn::visit::visit_item_foreign_mod(self, node);
            }
        }

        fn visit_foreign_item(&mut self, node: &'ast syn::ForeignItem) {
            if let Some(ident) = foreign_item_ident(node) {
                self.regions.declare(self.region, ident.to_string());
            }
        }

        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            self.uses.push(CollectedUse {
                item: node.clone(),
                context: self.context.clone(),
                inline_path: Arc::clone(&self.inline_path),
                region: self.region,
            });
        }

        fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
            let prev_context = self.context.clone();
            if is_cfg_test(&node.attrs) {
                self.context = self.context.promoted_to_test();
            }
            syn::visit::visit_impl_item_fn(self, node);
            self.context = prev_context;
        }

        impl_binding_region_visits!();
    }
    let mut collector = UseCollector {
        uses: Vec::new(),
        context: base_context,
        inline_path: Arc::from(""),
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
    /// As [`CollectedUse::inline_path`].
    pub(crate) inline_path: Arc<str>,
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
    /// Written in an attribute whose path starts with a registered tool
    /// ([`TOOL_NAMES`]), such as `#[rustfmt::skip]`.
    pub(crate) in_tool_attribute: bool,
}

/// The tools whose name starts an attribute path rustc reads as the tool's,
/// unless a crate of the extern prelude has the name.
const TOOL_NAMES: [&str; 5] = ["clippy", "diagnostic", "miri", "rust_analyzer", "rustfmt"];

/// The attributes rustc builds in that may stand on an item and expand to no
/// item, from the Rust Reference's index of built-in attributes.
const BUILT_IN_ATTRIBUTES: [&str; 39] = [
    "allow",
    "automatically_derived",
    "cold",
    "collapse_debuginfo",
    "coverage",
    "debugger_visualizer",
    "deny",
    "deprecated",
    "doc",
    "expect",
    "export_name",
    "forbid",
    "global_allocator",
    "ignore",
    "inline",
    "instruction_set",
    "link",
    "link_name",
    "link_ordinal",
    "link_section",
    "macro_export",
    "macro_use",
    "must_use",
    "naked",
    "no_link",
    "no_mangle",
    "non_exhaustive",
    "panic_handler",
    "path",
    "proc_macro",
    "proc_macro_attribute",
    "proc_macro_derive",
    "repr",
    "should_panic",
    "target_feature",
    "test",
    "track_caller",
    "used",
    "warn",
];

/// The derives rustc builds in, which implement a trait and define no name.
const BUILT_IN_DERIVES: [&str; 9] = [
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "Eq",
    "Hash",
    "Ord",
    "PartialEq",
    "PartialOrd",
];

/// Whether one of `attrs` is an attribute or derive macro rustc does not
/// build in, inside a `cfg_attr` too.
pub(crate) fn attributes_expand(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| meta_expands(&attr.meta))
}

/// Whether the attribute `meta` is a macro rustc does not build in, or
/// derives one.
fn meta_expands(meta: &syn::Meta) -> bool {
    let path = meta.path();
    let first = path
        .segments
        .first()
        .map(|segment| segment.ident.to_string());
    if first
        .as_deref()
        .is_some_and(|tool| TOOL_NAMES.contains(&tool))
    {
        return false;
    }
    let Some(name) = path.get_ident().map(ToString::to_string) else {
        return true;
    };
    let nested = || {
        meta.require_list().ok().and_then(|list| {
            list.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
                .ok()
        })
    };
    match name.as_str() {
        "cfg" => false,
        // `cfg_attr(predicate, attributes…)`: the predicate is no macro.
        "cfg_attr" => nested().is_none_or(|metas| metas.iter().skip(1).any(meta_expands)),
        "derive" => {
            nested().is_none_or(|metas| metas.iter().any(|meta| !is_built_in_derive(meta.path())))
        }
        "unsafe" => nested().is_none_or(|metas| metas.iter().any(meta_expands)),
        _ => !BUILT_IN_ATTRIBUTES.contains(&name.as_str()),
    }
}

/// Whether the attribute `meta` derives a macro rustc does not build in,
/// inside a `cfg_attr` too: a derive macro may declare helper attributes.
fn derives_a_macro(meta: &syn::Meta) -> bool {
    let nested = || {
        meta.require_list().ok().and_then(|list| {
            list.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
                .ok()
        })
    };
    if meta.path().is_ident("derive") {
        meta_expands(meta)
    } else if meta.path().is_ident("cfg_attr") {
        nested().is_none_or(|metas| metas.iter().skip(1).any(derives_a_macro))
    } else {
        false
    }
}

/// Whether the derive `path` names is one rustc builds in, written bare or
/// from `std` or `core`.
fn is_built_in_derive(path: &syn::Path) -> bool {
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let from_std = match segments.as_slice() {
        [_] => true,
        [first, ..] => first == "std" || first == "core",
        [] => false,
    };
    from_std
        && segments
            .last()
            .is_some_and(|name| BUILT_IN_DERIVES.contains(&name.as_str()))
}

impl PathRef {
    /// Whether this path names a tool rather than a crate: written in a tool
    /// attribute whose tool no crate of the extern prelude of `ctx`'s build
    /// shadows. rustc reads a crate of the tool's name first.
    fn names_a_tool(&self, ctx: &ResolutionContext) -> bool {
        let first = self.path.split("::").next().unwrap_or_default();
        self.in_tool_attribute
            && !ctx
                .crate_view
                .extern_prelude
                .names(ctx.test_build)
                .contains_key(first)
    }

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
            &self.inline_path,
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
            &self.inline_path,
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
    inline_path: Arc<str>,
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
    /// Inside an attribute whose path starts with a tool name.
    in_tool_attribute: bool,
    /// Inside an item that derives a macro rustc does not build in, whose
    /// helper attributes a bare attribute path may name.
    in_derive_helpers_reach: bool,
}
impl PathRefCollector {
    /// Visit an item that may carry derives with `attrs`, its fields and
    /// variants by `visit`, as [`PathRefCollector::in_derive_helpers_reach`]
    /// tells.
    fn under_derives(&mut self, attrs: &[syn::Attribute], visit: impl FnOnce(&mut Self)) {
        let prev = self.in_derive_helpers_reach;
        self.in_derive_helpers_reach = attrs.iter().any(|attr| derives_a_macro(&attr.meta));
        visit(self);
        self.in_derive_helpers_reach = prev;
    }

    /// Record the paths in the tokens of the attribute list `node`: a name in
    /// `#[derive(…)]` is a derive macro.
    fn record_meta_list_tokens(&mut self, node: &syn::MetaList) {
        let meaning = if node.path.is_ident("derive") {
            Meaning::Macro
        } else {
            Meaning::Any
        };
        self.record_token_paths(&node.tokens, meaning);
    }

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
            inline_path: Arc::clone(&self.inline_path),
            region: self.region,
            position: self.position,
            meaning,
            in_macro,
            local,
            in_tool_attribute: self.in_tool_attribute,
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
                // A lifetime is a `'` followed by an identifier, and a field
                // or method follows a `.`; neither is a name.
                TokenTree::Ident(ident)
                    if !is_lifetime_tick(tokens.get(index.wrapping_sub(1)))
                        && !follows_a_field_dot(&tokens[..index]) =>
                {
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

/// Whether the tokens before an identifier end with the `.` of a field or
/// method access. Two dots are a range, and a path may follow them.
fn follows_a_field_dot(before: &[proc_macro2::TokenTree]) -> bool {
    let is_dot = |token: &proc_macro2::TokenTree| matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '.');
    match before {
        [.., previous, dot] => is_dot(dot) && !is_dot(previous),
        [dot] => is_dot(dot),
        [] => false,
    }
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
            self.context = self.context.promoted_to_test();
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
        self.record_meta_list_tokens(node);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        self.under_derives(&node.attrs, |s| syn::visit::visit_item_struct(s, node));
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        self.under_derives(&node.attrs, |s| syn::visit::visit_item_enum(s, node));
    }

    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        self.under_derives(&node.attrs, |s| syn::visit::visit_item_union(s, node));
    }

    // `pub(in path)` names the scope that may see the item; nothing is used
    // from that module, so its path is no dependency.
    fn visit_vis_restricted(&mut self, _node: &'ast syn::VisRestricted) {}

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        let segments = &node.path().segments;
        self.in_tool_attribute = segments.len() > 1
            && segments
                .first()
                .is_some_and(|tool| TOOL_NAMES.iter().any(|name| tool.ident == name));
        // The attribute's own path names a macro; the value of `name = value`
        // is an expression. A bare path where a derive macro may declare
        // helper attributes may name a helper, which is no macro.
        let helper = self.in_derive_helpers_reach && segments.len() == 1;
        match &node.meta {
            syn::Meta::NameValue(name_value) => {
                if !helper {
                    self.naming_macro = true;
                    self.visit_path(&name_value.path);
                    self.naming_macro = false;
                }
                self.visit_expr(&name_value.value);
            }
            syn::Meta::List(list) if helper => self.record_meta_list_tokens(list),
            syn::Meta::Path(_) if helper => {}
            meta => {
                self.naming_macro = true;
                self.visit_meta(meta);
                self.naming_macro = false;
            }
        }
        self.in_tool_attribute = false;
    }

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
        inline_path: Arc::from(""),
        regions: BindingRegions::default(),
        region: BindingRegions::ROOT,
        position: PathPosition::Value,
        locals: Vec::new(),
        declared_later: None,
        naming_macro: false,
        in_tool_attribute: false,
        in_derive_helpers_reach: false,
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

/// `path` followed by `rest`, `path` alone where `rest` is empty.
fn followed_by(path: &str, rest: &str) -> String {
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
fn resolve_relative_path(path: &str, current_module_path: &str) -> Option<String> {
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

/// Resolve a single path written in the file of `ctx`: where it leads
/// ([`PathEnd::of_path`]), and whether it means the module it may end on or
/// the item of that name beside it. `meaning` is what the reference asks of
/// the path's last segment.
pub(crate) fn resolve_single_path(
    ctx: &ResolutionContext,
    path: &str,
    line_num: usize,
    context: &EdgeContext,
    inline_path: &str,
    meaning: Meaning,
) -> Option<DependencyRef> {
    let end = PathEnd::of_path(ctx, path, inline_path)?;
    Some(
        end.module_or_item(ctx, meaning)
            .dependency(ctx, line_num, context),
    )
}

/// The segments of the module path `path`, none for the crate root (`""`).
fn module_segments(path: &str) -> impl Iterator<Item = &str> {
    path.split("::").filter(|segment| !segment.is_empty())
}

/// What the first name of a path stands for in the module the path is
/// written in, as far as the one who asks can tell.
enum FirstName {
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
enum PickedStart<'p> {
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
    fn of(
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

/// Where a path leads: the package, the module in it (`""` for its root),
/// the item the path names there, and the root the package is read through.
/// No item where the path names the module itself.
#[derive(Debug)]
struct PathEnd {
    package: String,
    module: String,
    item: Option<String>,
    target_root: TargetRoot,
}

impl PathEnd {
    /// The root of the crate of `ctx`, as `crate::` names it.
    fn own_root(ctx: &ResolutionContext) -> Self {
        PathStart::own_crate(ctx).end((String::new(), None))
    }

    /// Where `path`, written in the file of `ctx` inside the inline modules
    /// `inline_path`, leads. Inside an inline module the path is read from
    /// that module, as from a file of its own. [`PickedStart::of`] picks the
    /// start, with what the module binds as the binding table has it
    /// ([`PathEnd::first_name`]); from there [`PathStart::follow`] walks the
    /// rest. A glob `base::*` leads where `base` does, with `*` as the item.
    /// `None` where no start fits, or where the walk enters a crate outside
    /// the workspace.
    fn of_path(ctx: &ResolutionContext, path: &str, inline_path: &str) -> Option<Self> {
        if !inline_path.is_empty() {
            let module = append_to_path(ctx.current_module_path, inline_path);
            return Self::of_path(&ctx.in_module(&module), path, "");
        }
        // `crate` alone, as in `use crate as name;`, is the root.
        if path == "crate" {
            return Some(Self::own_root(ctx));
        }
        // `crate::*` has no module to resolve and is the root's glob.
        if let Some(base) = path.strip_suffix("::*")
            && resolve_relative_path(path, ctx.current_module_path).is_none()
        {
            let base = if base == "crate" {
                Self::own_root(ctx)
            } else {
                Self::of_path(ctx, base, inline_path)?
            };
            return Some(base.glob());
        }
        let extern_prelude = ctx.crate_view.extern_prelude.names(ctx.test_build);
        let picked = PickedStart::of(
            path,
            ctx.current_module_path,
            extern_prelude,
            |first, after| Self::first_name(ctx, first, after),
        )?;
        let own = PathStart::own_crate(ctx);
        match picked {
            PickedStart::OwnRoot(from_root) => {
                let segments: Vec<&str> = from_root.split("::").collect();
                own.follow(ctx, &segments)
            }
            PickedStart::Module {
                crate_name,
                path,
                rest,
            } if crate_name == own.crate_name => {
                let in_crate: Vec<&str> = module_segments(&path)
                    .chain(module_segments(rest))
                    .collect();
                Some(own.walk(ctx, &in_crate))
            }
            PickedStart::Module {
                crate_name,
                path,
                rest,
            } => {
                let in_other: Vec<&str> = std::iter::once(crate_name.as_str())
                    .chain(module_segments(&path))
                    .chain(module_segments(rest))
                    .collect();
                Self::in_workspace_crate(ctx, &in_other.join("::"))
            }
            PickedStart::TypeItem { name } => {
                Some(own.end((ctx.current_module_path.to_string(), Some(name.to_string()))))
            }
            PickedStart::Crate { crate_name, rest } => {
                Self::in_workspace_crate(ctx, &followed_by(&crate_name, rest))
            }
            PickedStart::Unknown { .. } | PickedStart::Outside => {
                Self::in_external_crate(ctx, path)
            }
        }
    }

    /// What `first`, a path goes on past as `after` says, stands for in the
    /// module of `ctx`, as the binding table has it: a child module, a module
    /// a `use` or glob binds, an item of the type namespace the module
    /// defines.
    fn first_name(ctx: &ResolutionContext, first: &str, after: AfterName) -> FirstName {
        let own = PathStart::own_crate(ctx);
        if ctx.has_child_module(first) {
            return FirstName::Module {
                path: append_to_path(ctx.current_module_path, first),
                crate_name: own.crate_name,
            };
        }
        let module = CrateModule {
            crate_name: &own.crate_name,
            path: ctx.current_module_path,
        };
        let key = (
            ctx.test_build,
            ctx.current_module_path.to_string(),
            first.to_string(),
            after,
        );
        let cached = ctx.crate_view.modules_bound.0.borrow().get(&key).cloned();
        let lookup = ctx.lookup(own.target_root);
        let named = cached.unwrap_or_else(|| {
            let named = lookup.module_named(module, first, after);
            ctx.crate_view
                .modules_bound
                .0
                .borrow_mut()
                .insert(key, named.clone());
            named
        });
        if let Some(NameTarget::Module { crate_name, path }) = named {
            return FirstName::Module { crate_name, path };
        }
        let defines_a_type = after == AfterName::PathGoesOn
            && lookup.viewer.bindings_in(module).is_some_and(|bindings| {
                bindings.named(first).any(|binding| {
                    binding.definition().is_some() && binding.namespace() == Some(Namespace::Type)
                })
            });
        if defines_a_type {
            FirstName::TypeItem
        } else {
            FirstName::Unbound
        }
    }

    /// Where `path`, written from the root of `package`, a workspace crate
    /// other than this one or an external crate, leads. No name of this crate
    /// is asked: the package is known. A glob keeps its `*` as the item.
    fn in_package(ctx: &ResolutionContext, package: &str, path: &str) -> Option<Self> {
        if let Some(base) = path
            .strip_suffix("::*")
            .or_else(|| (path == "*").then_some(""))
        {
            return Some(Self::in_package(ctx, package, base)?.glob());
        }
        let full = if path.is_empty() {
            package.to_string()
        } else {
            format!("{package}::{path}")
        };
        Self::in_workspace_crate(ctx, &full).or_else(|| Self::in_external_crate(ctx, &full))
    }

    /// Where a path through a workspace crate leads, `other_crate::module`
    /// or `other_crate` alone for its root.
    fn in_workspace_crate(ctx: &ResolutionContext, path: &str) -> Option<Self> {
        let segments: Vec<&str> = path.split("::").collect();
        let (crate_name, rest) = segments.split_first()?;
        if !ctx.crate_view.workspace_crates.contains(crate_name) {
            return None;
        }
        let start = PathStart::named_crate(ctx, crate_name);
        if rest.is_empty() {
            return Some(start.end((String::new(), None)));
        }
        start.follow(ctx, rest)
    }

    /// Where a path through an external crate leads: `use serde::Deserialize`
    /// names `Deserialize` of `serde`. What the crate holds is unknown, so
    /// everything after its name is the item.
    fn in_external_crate(ctx: &ResolutionContext, path: &str) -> Option<Self> {
        let (first, rest) = path
            .split_once("::")
            .map_or((path, None), |(first, rest)| (first, Some(rest)));
        if first.is_empty() || !ctx.crate_view.external_crate_names.contains_key(first) {
            return None;
        }
        Some(Self {
            package: first.to_string(),
            module: String::new(),
            item: rest.map(ToString::to_string),
            target_root: TargetRoot::AsSeen,
        })
    }

    /// The glob of the module this end names, or of the module holding its
    /// item.
    fn glob(self) -> Self {
        Self {
            item: Some("*".to_string()),
            ..self
        }
    }

    /// Decide whether a path that ends on a module means the module or the
    /// item of the same name in the module's parent. Beside `mod helper; pub
    /// use helper::helper;` the path `parent::helper` names the module and
    /// the fn. A call or a value means the fn, where the parent holds one; an
    /// item the parent does not show keeps the module. Otherwise only the fn
    /// is visible outside `parent`: a file that sees the module, as its
    /// declaration says from the file's crate, root and module, keeps the
    /// module. A re-export of a module under that name keeps the module, and
    /// so does a public module, which is visible wherever the item is.
    fn module_or_item(self, ctx: &ResolutionContext, meaning: Meaning) -> Self {
        if self.item.is_some() || self.module.is_empty() {
            return self;
        }
        let (parent, name) = self
            .module
            .rsplit_once("::")
            .unwrap_or(("", self.module.as_str()));
        let lookup = ctx.lookup(self.target_root);
        let parent_module = CrateModule {
            crate_name: &self.package,
            path: parent,
        };
        let item_asked =
            !meaning.admits_a_module() && lookup.holds_an_item_named(parent_module, name);
        let item_meant = item_asked
            || lookup
                .viewer
                .bindings_in(parent_module)
                .filter(|bindings| !sees_the_module(*bindings, name))
                .and_then(|bindings| bindings.named(name).find_map(NameBinding::reexported))
                .is_some_and(|target| !lookup.binds_a_module(target));
        if !item_meant {
            return self;
        }
        let (parent, name) = (parent.to_string(), name.to_string());
        Self {
            module: parent,
            item: Some(name),
            ..self
        }
    }

    /// The dependency of the reference at `line_num` in the file of `ctx` on
    /// this end.
    fn dependency(
        self,
        ctx: &ResolutionContext,
        line_num: usize,
        context: &EdgeContext,
    ) -> DependencyRef {
        DependencyRef {
            target_package: self.package,
            target_module: self.module,
            target_item: self.item,
            source_file: ctx.source_file.to_path_buf(),
            line: line_num,
            context: context.clone(),
            via_reexport: false,
            uses: Vec::new(),
            target_root: self.target_root,
        }
    }
}

/// The root of a workspace crate a path starts at, and the root of the
/// crate's build it is read through.
struct PathStart {
    crate_name: String,
    target_root: TargetRoot,
    /// Whether the path names the crate, rather than writing `crate::` or a
    /// relative path in it. Its root's names then also come from
    /// `crate_exports`, which reads the root file alone.
    by_name: bool,
}

impl PathStart {
    /// The root of the crate of `ctx`, as `crate::` names it.
    fn own_crate(ctx: &ResolutionContext) -> Self {
        Self {
            crate_name: normalize_crate_name(ctx.crate_view.crate_name),
            target_root: TargetRoot::AsSeen,
            by_name: false,
        }
    }

    /// The root of the workspace crate `crate_name` names. The own package by
    /// name is its lib, also from a file of the bin tree.
    fn named_crate(ctx: &ResolutionContext, crate_name: &str) -> Self {
        let crate_name = normalize_crate_name(crate_name);
        let target_root = if crate_name == normalize_crate_name(ctx.crate_view.crate_name) {
            TargetRoot::Lib
        } else {
            TargetRoot::AsSeen
        };
        Self {
            crate_name,
            target_root,
            by_name: true,
        }
    }

    /// Where `segments`, written from this root, lead. The first is asked of
    /// the root's names ([`NameLookup::name`]): a module carries the rest of
    /// the path into it, also in another workspace crate; an item, or a module
    /// of another crate the path ends on, is the item of the root. A name the
    /// root does not bind starts down the module tree ([`PathStart::walk`]),
    /// unless the path names the crate and its root file names the name.
    fn follow(&self, ctx: &ResolutionContext, segments: &[&str]) -> Option<PathEnd> {
        let (&first, rest) = segments.split_first()?;
        if first.is_empty() {
            return None;
        }
        let root = CrateModule {
            crate_name: &self.crate_name,
            path: "",
        };
        let named = ctx
            .lookup(self.target_root)
            .name(root, first, AfterName::followed_by(rest));
        let item_in_root = (String::new(), Some(first.to_string()));
        let reached = match named {
            Some(NameTarget::Module { crate_name, path }) if crate_name != self.crate_name => {
                if rest.is_empty() {
                    item_in_root
                } else {
                    let in_other: Vec<&str> = std::iter::once(crate_name.as_str())
                        .chain(module_segments(&path))
                        .chain(rest.iter().copied())
                        .collect();
                    return PathEnd::in_workspace_crate(ctx, &in_other.join("::"));
                }
            }
            Some(NameTarget::Module { path, .. }) => {
                let in_crate: Vec<&str> =
                    module_segments(&path).chain(rest.iter().copied()).collect();
                if in_crate.is_empty() {
                    item_in_root
                } else {
                    return Some(self.walk(ctx, &in_crate));
                }
            }
            Some(NameTarget::Item) => item_in_root,
            None if self.by_name && self.root_file_names(ctx, first) => item_in_root,
            None => return Some(self.walk(ctx, segments)),
        };
        Some(self.end(reached))
    }

    /// Whether the root file of this crate names `name`, as `crate_exports`
    /// has it.
    fn root_file_names(&self, ctx: &ResolutionContext, name: &str) -> bool {
        ctx.crate_view
            .crate_exports
            .get(&self.crate_name)
            .is_some_and(|exports| exports.contains(name))
    }

    /// Where `segments` lead from this root, one segment at a time. A segment
    /// that names a child module of the module reached so far enters it.
    /// Another segment the path goes on past is asked of the module's names,
    /// and a module below this crate's root it stands for carries the rest:
    /// beside `use crate::store::{self as bound};` in `holder`,
    /// `holder::bound::Set` names `Set` in `store`. A module of another
    /// workspace crate carries the rest into that crate. The first segment
    /// that carries the path no further is the item.
    fn walk(&self, ctx: &ResolutionContext, segments: &[&str]) -> PathEnd {
        let module_paths = ctx.crate_view.module_paths.get_or_empty(&self.crate_name);
        let lookup = ctx.lookup(self.target_root);
        let mut visited = Visited::default();
        let mut module = String::new();
        let mut rest = segments;
        loop {
            while let Some((first, tail)) = rest.split_first() {
                let child = append_to_path(&module, first);
                if !module_paths.contains(&child) {
                    break;
                }
                module = child;
                rest = tail;
            }
            let Some((&name, tail)) = rest.split_first() else {
                return self.end((module, None));
            };
            let item = (!name.is_empty()).then(|| name.to_string());
            if tail.is_empty() {
                return self.end((module, item));
            }
            let holder = CrateModule {
                crate_name: &self.crate_name,
                path: &module,
            };
            match lookup.name_passing(holder, name, AfterName::PathGoesOn, &mut visited) {
                Some(NameTarget::Module { crate_name, path })
                    if crate_name == self.crate_name && !path.is_empty() =>
                {
                    module = path;
                    rest = tail;
                }
                Some(NameTarget::Module { crate_name, path }) if crate_name != self.crate_name => {
                    let in_other: Vec<&str> = std::iter::once(crate_name.as_str())
                        .chain(module_segments(&path))
                        .chain(tail.iter().copied())
                        .collect();
                    return PathEnd::in_workspace_crate(ctx, &in_other.join("::"))
                        .unwrap_or_else(|| self.end((module, item)));
                }
                Some(NameTarget::Module { .. } | NameTarget::Item) | None => {
                    return self.end((module, item));
                }
            }
        }
    }

    /// The end at `reached`, a module of this crate and the item there.
    fn end(&self, (module, item): (String, Option<String>)) -> PathEnd {
        PathEnd {
            package: self.crate_name.clone(),
            module,
            item,
            target_root: self.target_root,
        }
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

        // A `use` binds every meaning of its target. `use crate as name;`
        // only names the crate's own root for the paths through `name`.
        for path in paths.into_iter().filter(|path| path != "crate") {
            if let Some(mut dep) = bindings.resolve_use_path(ctx, &path, collected) {
                // A private glob whose target the map knows is resolved into
                // the names the file uses, by `parse_path_ref_dependencies`.
                // A `pub use` glob republishes every name and uses none, so it
                // is the whole payload.
                if is_glob(&dep) && glob_payload_known(&dep, ctx) && !via_reexport {
                    continue;
                }
                dep.via_reexport = via_reexport;
                for mut dep in expand_glob(dep, ctx, &collected.inline_path) {
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
///
/// A name the module holding the glob, in the inline modules `inline_path`
/// of the file, binds itself in the namespace the glob brings it in is not
/// the glob's: its own binding hides the glob there. Where either namespace
/// is unknown, the glob keeps the name.
fn expand_glob(
    dep: DependencyRef,
    ctx: &ResolutionContext,
    inline_path: &str,
) -> Vec<DependencyRef> {
    if !is_glob(&dep) {
        return vec![dep];
    }
    let Some(names) = glob_payload(&dep, ctx) else {
        return vec![dep];
    };
    let own = PathStart::own_crate(ctx);
    let holder_path = append_to_path(ctx.current_module_path, inline_path);
    let holder = CrateModule {
        crate_name: &own.crate_name,
        path: &holder_path,
    };
    let lookup = ctx.lookup(own.target_root);
    let source_lookup = ctx.lookup_for(&dep);
    let bound_by_the_holder = |name: &str| {
        let Some(brought) = source_lookup.definition_namespace(
            CrateModule::of(&dep),
            name,
            &mut Visited::default(),
        ) else {
            return false;
        };
        lookup.definition_namespace(holder, name, &mut Visited::default()) == Some(brought)
    };
    names
        .into_iter()
        .filter(|name| !bound_by_the_holder(name))
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
/// module or a crate root by `pub use` or, where the file sits below it in the same crate, by
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
        for (collected, binding, original_path) in use_bindings(use_items) {
            if binding == "*" && original_path == "super::*" {
                bindings.regions.see_around(collected.region);
            }
        }

        // A `use` path may start with a module another `use` or a glob binds,
        // written before or after it. Those bindings come first; a chain
        // through two such bindings is followed one step only.
        for (collected, binding, original_path) in use_bindings(use_items) {
            let dep = resolve_single_path(
                ctx,
                &original_path,
                collected.item.use_token.span.start().line,
                &collected.context,
                &collected.inline_path,
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
            if self.regions.declares(enclosing, first) {
                break;
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
        let (context, inline_path) = (&collected.context, &*collected.inline_path);
        match self.binding_of(path, collected.region) {
            PathBinding::Module(module, rest) => {
                module.resolve(ctx, rest, line, context, inline_path, Meaning::Any)
            }
            _ => resolve_single_path(ctx, path, line, context, inline_path, Meaning::Any),
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
    /// the names that module binds to a module or a crate root by `use`, each
    /// where the file sees it from its crate, root and module.
    fn bind_glob_modules(
        &mut self,
        region: RegionId,
        ctx: &ResolutionContext,
        dep: &DependencyRef,
    ) {
        let lookup = ctx.lookup_for(dep);
        let module = CrateModule::of(dep);
        let bindings = lookup.viewer.bindings_in(module);
        let module_paths = ctx
            .crate_view
            .module_paths
            .get_or_empty(&dep.target_package);
        let children = module_paths.iter().filter_map(|path| {
            let (parent, name) = path.rsplit_once("::").unwrap_or(("", path));
            if parent != dep.target_module {
                return None;
            }
            lookup.declared_child(module, bindings, name)?;
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
            .filter_map(|(name, binding)| Some((name, binding.use_target()?)))
            .filter(|(_, target)| lookup.module_is_visible(target) && lookup.binds_a_module(target))
            .map(|(name, target)| {
                let bound = BoundModule {
                    package: target.crate_name.clone(),
                    path: append_to_path(&target.module, &target.original_name),
                    target_root: dep.target_root,
                    import: None,
                };
                (name.to_string(), bound)
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
                if let Some(ident) = item_ident(node) {
                    self.0.insert(ident.to_string());
                }
                syn::visit::visit_item(self, node);
            }

            fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
                if !attributes_expand(&node.attrs) {
                    syn::visit::visit_item_foreign_mod(self, node);
                }
            }

            fn visit_foreign_item(&mut self, node: &'ast syn::ForeignItem) {
                if let Some(ident) = foreign_item_ident(node) {
                    self.0.insert(ident.to_string());
                }
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

/// The name an item declares; none for `use`, `impl` and the like.
fn item_ident(item: &syn::Item) -> Option<&syn::Ident> {
    match item {
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
    }
}

/// The name an item of an `extern` block declares in the module holding the
/// block; none for a macro call and for an item under an attribute macro,
/// which may remove or rewrite it.
fn foreign_item_ident(item: &syn::ForeignItem) -> Option<&syn::Ident> {
    let (attrs, ident) = match item {
        syn::ForeignItem::Fn(i) => (&i.attrs, &i.sig.ident),
        syn::ForeignItem::Static(i) => (&i.attrs, &i.ident),
        syn::ForeignItem::Type(i) => (&i.attrs, &i.ident),
        _ => return None,
    };
    (!attributes_expand(attrs)).then_some(ident)
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
/// keeps a path from starting at a child module of that name.
///
/// A binding to a module, or to the root of another crate, is a module binding:
/// what follows the name resolves under that module.
///
/// A binding from another crate to an item needs an entry only where its name
/// also names a module of this crate. Without one, the reference lands on that
/// module. Whether it would is asked of [`ResolutionContext::has_child_module`],
/// which decides where a bare path starts, so the two cannot drift apart.
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
    if dep.target_package == normalize_crate_name(ctx.crate_view.crate_name) {
        return Binding::OwnItem;
    }
    if !binding.is_empty() && ctx.has_child_module(binding) {
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
        inline_path: &str,
        meaning: Meaning,
    ) -> Option<DependencyRef> {
        let path = append_to_path(&self.path, rest);
        if self.target_root == TargetRoot::AsSeen
            && self.package == normalize_crate_name(ctx.crate_view.crate_name)
        {
            let path = format!("crate::{path}");
            return resolve_single_path(ctx, &path, line_num, context, inline_path, meaning);
        }
        let end = PathEnd::in_package(ctx, &self.package, &path)?;
        Some(
            end.module_or_item(ctx, meaning)
                .dependency(ctx, line_num, context),
        )
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
        if path_ref.names_a_tool(ctx) {
            continue;
        }
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
                        if associated.is_some() && ends_on_an_item_named_like_its_crate(ctx, &dep) {
                            continue;
                        }
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
    let brought = |glob: &DependencyRef| ctx.lookup_for(glob).brings(CrateModule::of(glob), name);
    let brings = |glob: &DependencyRef| brought(glob).is_some();
    let brings_in_every_build = |glob: &DependencyRef| brought(glob) == Some(Brought::InEveryBuild);
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

/// Whether the re-export chain of `dep` ends on a `pub use` of an item of an
/// external crate named like the item, such as `pub use helper::helper;`.
/// rustc reads a path that goes on past a glob's name through the glob only
/// where the glob brings the name as a module or a type, and from the crate
/// of that name otherwise; which one the item is, the map does not know.
fn ends_on_an_item_named_like_its_crate(ctx: &ResolutionContext, dep: &DependencyRef) -> bool {
    let mut end = dep.clone();
    ctx.resolve_reexport(&mut end, Meaning::Type);
    let (Some(item), Some(bindings)) = (&end.target_item, ctx.target_bindings(&end)) else {
        return false;
    };
    bindings.named(item).any(|binding| match &binding.kind {
        BindingKind::ReExportFromOutside(path) => path
            .split_once("::")
            .is_some_and(|(crate_name, _)| crate_name == item),
        _ => false,
    })
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
/// of the name. A name that reaches a module or a crate root declares one
/// too: no pattern matches a module.
fn declares_a_variable(ctx: &ResolutionContext, dep: &DependencyRef, meaning: Meaning) -> bool {
    let mut probed = dep.clone();
    !ctx.resolve_reexport(&mut probed, meaning)
        .places_no_definer()
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
///
/// Each piece of code resolves in the build its context reads
/// ([`RootBuilds::reading`]), whatever build `file` reads.
pub(crate) fn parse_file_dependencies(
    syntax: &syn::File,
    file: &ResolutionContext,
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
    let production_ctx = file.reading(UsageKind::Production);
    if !holds_test_code {
        return resolve_file_dependencies(
            syntax,
            &production_ctx,
            &use_items,
            &use_items,
            &path_refs,
            passed_on,
        );
    }
    // Production code resolves in the build without `test`, which binds
    // none of the test code's `use` items, and test code in the build its
    // context reads. Each pass notes in `passed_on` only what it keeps. A
    // file of test code alone, such as one only test builds load, has no
    // production pass.
    let production_uses = use_items.retaining(is_production);
    let production_refs = path_refs.retaining(is_production);
    let holds_production_code =
        production_uses.iter().next().is_some() || production_refs.iter().next().is_some();
    let mut deps = if holds_production_code {
        resolve_file_dependencies(
            syntax,
            &production_ctx,
            &production_uses,
            &production_uses,
            &production_refs,
            passed_on,
        )
    } else {
        Vec::new()
    };
    if !include_tests {
        return deps;
    }
    // Every kind of test code reads the same build.
    let test_ctx = file.reading(UsageKind::Test(TestKind::Unit));
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

#[cfg(test)]
mod tests;
