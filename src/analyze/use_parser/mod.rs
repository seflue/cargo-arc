//! Syn-based use statement parsing for workspace dependency extraction.

use super::packages::{ModulePathMap, PackageExportMap, WorkspacePackages, normalize_crate_name};
use crate::model::{
    DefKind, Definition, DependencyRef, EdgeContext, EntryKind, SymbolUse, TestKind, UsageKind,
    UseCategory,
};
use std::borrow::Cow;
use std::cell::RefCell;
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
    /// Where the `use` that binds the name is visible.
    pub(crate) visibility: Visibility,
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

/// Export and re-export information for a single module.
#[derive(Debug, Default, Clone)]
pub(crate) struct ModuleExportInfo {
    /// Own definitions declared `pub`, `pub(crate)` or `pub(super)`: name →
    /// kind and line at the definition site
    pub(crate) definitions: HashMap<String, Definition>,
    /// Where each item defined here is visible, by name, the items outside
    /// `definitions` included. In the root, every module of the crate can
    /// name a private item through `crate::`.
    pub(crate) item_visibility: HashMap<String, Visibility>,
    /// Where each child module declared here is visible, by name. A glob or
    /// a path from where a module is not visible does not reach it.
    pub(crate) module_visibility: HashMap<String, Visibility>,
    /// The associated items of the types and traits defined here, by the
    /// defined name. An `impl` block in another file of the crate lands here
    /// too, on the module that defines the type.
    pub(crate) associated: HashMap<String, AssociatedItems>,
    /// Explicit re-exports, `pub`, `pub(crate)` or `pub(super)`: alias/name →
    /// source target
    pub(crate) explicit_reexports: HashMap<String, ReExportTarget>,
    /// Private, `pub(self)` and `pub(in …)` `use` bindings this module holds.
    /// Rust makes them visible to descendant modules, which may then name the
    /// symbol through this module even though it defines nothing. Resolving
    /// through them attributes the edge to the definer, not to this ancestor.
    /// Name → source target.
    pub(crate) private_uses: HashMap<String, ReExportTarget>,
    /// Glob re-export sources (module paths from `pub use *`, `pub(crate)`
    /// and `pub(super)` included)
    pub(crate) glob_sources: Vec<GlobSource>,
    /// Glob re-export sources in another workspace crate
    /// (`pub use other_crate::m::*`): crate name and module path there.
    pub(crate) foreign_glob_sources: Vec<(String, GlobSource)>,
    /// Private glob sources (module paths from a private, `pub(self)` or
    /// `pub(in …)` `use *`). Like `private_uses`, visible to descendants,
    /// which may name any of the forwarded symbols through this module.
    pub(crate) private_glob_sources: Vec<GlobSource>,
    /// Private glob sources in another workspace crate
    /// (`use other_crate::m::*`): crate name and module path there. Visible
    /// to descendants like `private_glob_sources`.
    pub(crate) foreign_private_glob_sources: Vec<(String, GlobSource)>,
    /// Names a private `use` binds to a path the resolution chain cannot
    /// place, with where the `use` is visible: an external crate, invisible
    /// while re-exports are collected. Visible to descendants like
    /// `private_uses`, but nothing here names a definer, so a descendant's
    /// reference through this module produces no edge rather than staying on
    /// the ancestor.
    pub(crate) elsewhere: HashMap<String, Visibility>,
}

impl ModuleExportInfo {
    /// Whether the module holds nothing a resolution reads. A public item or
    /// child module is visible wherever the module is, as it is when the
    /// module holds no entry.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        let public = |visibility: &Visibility| *visibility == Visibility::Public;
        self.definitions.is_empty()
            && self.item_visibility.values().all(public)
            && self.module_visibility.values().all(public)
            && self.associated.is_empty()
            && self.explicit_reexports.is_empty()
            && self.private_uses.is_empty()
            && self.glob_sources.is_empty()
            && self.foreign_glob_sources.is_empty()
            && self.private_glob_sources.is_empty()
            && self.foreign_private_glob_sources.is_empty()
            && self.elsewhere.is_empty()
    }
}

/// What one crate root's module tree exports: module path → export info.
/// Module paths are crate-relative (e.g. `"render"`, `"analyze::use_parser"`),
/// and `""` is the root file itself.
#[derive(Debug, Clone)]
pub(crate) struct RootReExports {
    pub(crate) kind: EntryKind,
    pub(crate) modules: HashMap<String, ModuleExportInfo>,
}

/// Crate name → one [`RootReExports`] per entry file, in `entry_files` order.
///
/// A lib and a bin root of one package share the module tree but not their
/// root's exports. Another package sees only the lib root, the only one it can
/// depend on. The package of the file being resolved sees the root whose tree
/// holds the file, set with [`ReExportMap::for_root`].
#[derive(Debug, Default, Clone)]
pub(crate) struct ReExportMap {
    packages: Arc<HashMap<String, Vec<RootReExports>>>,
    own_root: Option<(String, usize)>,
}

impl ReExportMap {
    /// The same map, seen from the `root`-th entry file of `crate_name`.
    pub(crate) fn for_root(&self, crate_name: &str, root: usize) -> Self {
        Self {
            packages: Arc::clone(&self.packages),
            own_root: Some((crate_name.to_string(), root)),
        }
    }

    /// The same map, seen from outside every package: each sees its lib root.
    /// A bin root that names its own package by name names the lib.
    pub(crate) fn lib_view(&self) -> Self {
        Self {
            packages: Arc::clone(&self.packages),
            own_root: None,
        }
    }

    /// The export info of `crate_name`'s modules, as this map sees them.
    pub(crate) fn get(&self, crate_name: &str) -> Option<&HashMap<String, ModuleExportInfo>> {
        self.modules_of(crate_name, false)
    }

    /// The export info of the modules of the crate a dependency points at, in
    /// the view the dependency names them in: a dependency in the lib view
    /// sees the lib root of its own package too.
    pub(crate) fn exports_for(
        &self,
        dep: &DependencyRef,
    ) -> Option<&HashMap<String, ModuleExportInfo>> {
        self.modules_of(&dep.target_package, dep.in_lib_view)
    }

    fn modules_of(
        &self,
        crate_name: &str,
        in_lib_view: bool,
    ) -> Option<&HashMap<String, ModuleExportInfo>> {
        let roots = self.packages.get(crate_name)?;
        let root = match &self.own_root {
            Some((own, index)) if !in_lib_view && own == crate_name => roots.get(*index)?,
            _ => roots.iter().find(|root| root.kind == EntryKind::Lib)?,
        };
        Some(&root.modules)
    }

    /// Look up the export info of the module a dependency points at, where
    /// the map has it, in the view the dependency names it in.
    pub(crate) fn module_info(&self, dep: &DependencyRef) -> Option<&ModuleExportInfo> {
        self.exports_for(dep)?.get(&dep.target_module)
    }

    /// Whether a binding with `visibility` in `crate_name`, reached in the lib
    /// view or not, is visible from `module` of the crate root this map is
    /// seen from. A restricted binding is visible from below its module in
    /// the same crate only: another package, another root of the same
    /// package and a map seen from outside every package see what is public.
    pub(crate) fn is_visible(
        &self,
        crate_name: &str,
        in_lib_view: bool,
        visibility: &Visibility,
        module: &str,
    ) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Restricted(ancestor) => {
                self.is_own_root(crate_name, in_lib_view) && is_descendant(module, ancestor)
            }
        }
    }

    /// Whether the modules of `crate_name`, reached in the lib view or not,
    /// are those of the root this map is seen from.
    fn is_own_root(&self, crate_name: &str, in_lib_view: bool) -> bool {
        let Some((own, index)) = &self.own_root else {
            return false;
        };
        own == crate_name
            && (!in_lib_view
                || self
                    .packages
                    .get(own)
                    .and_then(|roots| roots.get(*index))
                    .is_some_and(|root| root.kind == EntryKind::Lib))
    }
}

impl FromIterator<(String, Vec<RootReExports>)> for ReExportMap {
    fn from_iter<I: IntoIterator<Item = (String, Vec<RootReExports>)>>(iter: I) -> Self {
        Self {
            packages: Arc::new(iter.into_iter().collect()),
            own_root: None,
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
                };
                (crate_name, vec![lib])
            })
            .collect()
    }
}

/// [`resolve_reexport_noting`] with nowhere to note what is passed on.
#[cfg(test)]
pub(crate) fn resolve_reexport(
    dep: &mut DependencyRef,
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
    importer_module: &str,
) -> bool {
    resolve_reexport_noting(
        dep,
        reexport_map,
        module_paths,
        importer_module,
        None,
        Meaning::Any,
    )
}

/// A module holding a private `use` or glob, by crate and module path.
type Holder = (String, String);

/// Where a name is defined: crate, module path and the name there.
type DefinedAt = (String, String, String);

/// The private `use` items and globs a descendant named something through,
/// per module that holds them. Such a module passes the names on to its
/// descendants. Each note records whether production code named something
/// through it, since only that passes a name on for production code.
#[derive(Debug, Default)]
pub(crate) struct PassedOn {
    /// Holder and the module its glob imports from.
    globs: RefCell<HashMap<(Holder, String), FromProduction>>,
    /// Holder and where the name the descendant reached is defined.
    uses: RefCell<HashMap<(Holder, DefinedAt), FromProduction>>,
}

/// Whether production code, not only test code, named something through a
/// note's holder.
type FromProduction = bool;

impl PassedOn {
    /// Note that the module `dep` now points at passed on what its private
    /// glob from `source` brings in.
    fn note_glob(&self, dep: &DependencyRef, source: &str) {
        let noted = (
            (dep.target_package.clone(), dep.target_module.clone()),
            source.to_string(),
        );
        *self.globs.borrow_mut().entry(noted).or_default() |=
            dep.context.kind == UsageKind::Production;
    }

    /// Note that each of `holders` other than `importer_module` itself passed
    /// `name`, which `dep` now points at the module of, on.
    fn note_uses(
        &self,
        holders: Vec<Holder>,
        importer_module: &str,
        dep: &DependencyRef,
        name: &str,
    ) {
        let mut uses = self.uses.borrow_mut();
        for holder in holders.into_iter().filter(|(_, m)| m != importer_module) {
            let defined_at = (
                dep.target_package.clone(),
                dep.target_module.clone(),
                name.to_string(),
            );
            *uses.entry((holder, defined_at)).or_default() |=
                dep.context.kind == UsageKind::Production;
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
                && covers(self.globs.borrow().get(&noted), dep)
                && !glob_may_bring_a_trait(
                    dep,
                    holder,
                    reexport_map,
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
        is_import_only(dep) && covers(self.uses.borrow().get(&noted), dep)
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

/// Whether the glob `dep` brings a trait into `holder`, or a name whose kind
/// the map does not know at its definer. A module the map does not hold
/// counts as unknown, and so does a name the chain follows out of the map.
fn glob_may_bring_a_trait(
    dep: &DependencyRef,
    holder: &str,
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    if !visited.insert((dep.target_package.clone(), dep.target_module.clone())) {
        return false;
    }
    let Some(info) = reexport_map.module_info(dep) else {
        return true;
    };
    let defines_a_trait = info
        .definitions
        .values()
        .any(|definition| definition.kind == DefKind::Trait);
    let reexports_a_trait = info
        .explicit_reexports
        .keys()
        .any(|name| reexports_a_trait_or_unknown(dep, name, holder, reexport_map, module_paths));
    let local_globs = info
        .glob_sources
        .iter()
        .map(|glob| (dep.target_package.clone(), glob.module.clone()));
    let foreign_globs = info
        .foreign_glob_sources
        .iter()
        .map(|(package, glob)| (package.clone(), glob.module.clone()));
    defines_a_trait
        || reexports_a_trait
        || local_globs.chain(foreign_globs).any(|(package, module)| {
            let source = DependencyRef {
                target_package: package,
                target_module: module,
                ..dep.clone()
            };
            glob_may_bring_a_trait(&source, holder, reexport_map, module_paths, visited)
        })
}

/// Whether `name`, re-exported by the module `dep` imports from, is a trait at
/// its definer, or the chain ends on no definition the map knows. A name that
/// ends on a module is no trait. Nothing in `holder` asks a meaning of the
/// name, so the chain follows any.
fn reexports_a_trait_or_unknown(
    dep: &DependencyRef,
    name: &str,
    holder: &str,
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
) -> bool {
    let mut named = DependencyRef {
        target_item: Some(name.to_string()),
        ..dep.clone()
    };
    if !resolve_reexport_noting(
        &mut named,
        reexport_map,
        module_paths,
        holder,
        None,
        Meaning::Any,
    ) {
        return true;
    }
    let Some(item) = &named.target_item else {
        return false;
    };
    reexport_map
        .module_info(&named)
        .and_then(|info| info.definitions.get(item))
        .is_none_or(|definition| definition.kind == DefKind::Trait)
}

/// Resolve re-exports in a [`DependencyRef`]: follow the re-export chain
/// until the original definition module is found, crossing into another
/// workspace crate where the chain does.
/// Modifies `dep.target_package` and `dep.target_module` in place; a no-op if
/// no re-export applies. Returns `false` when the chain instead reaches a
/// name a private `use` bound to something outside the crate (an external
/// crate) — the caller then produces no edge for `dep` — and `true` otherwise.
/// Each private glob and private `use` of an ancestor the chain passes
/// through is noted in `passed_on`. A `dep` in the lib view follows the chain
/// from its package's lib root. Whether a name binds a module, `module_paths`
/// tells; whether the reference means it, `meaning`.
fn resolve_reexport_noting(
    dep: &mut DependencyRef,
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
    importer_module: &str,
    passed_on: Option<&PassedOn>,
    meaning: Meaning,
) -> bool {
    if dep.in_lib_view {
        follow_reexports(
            dep,
            &reexport_map.lib_view(),
            module_paths,
            importer_module,
            passed_on,
            meaning,
        )
    } else {
        follow_reexports(
            dep,
            reexport_map,
            module_paths,
            importer_module,
            passed_on,
            meaning,
        )
    }
}

/// The chain of [`resolve_reexport_noting`], followed as `reexport_map` sees
/// it.
fn follow_reexports(
    dep: &mut DependencyRef,
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
    importer_module: &str,
    passed_on: Option<&PassedOn>,
    meaning: Meaning,
) -> bool {
    let Some(mut crate_exports) = reexport_map.get(&dep.target_package) else {
        return true;
    };
    let mut visited = HashSet::new();
    let mut lookup_name = dep.target_item.clone();
    let mut use_holders: Vec<Holder> = Vec::new();
    let in_lib_view = dep.in_lib_view;
    let visible = |crate_name: &str, visibility: &Visibility| {
        reexport_map.is_visible(crate_name, in_lib_view, visibility, importer_module)
    };
    let ends_on_module = |dep: &mut DependencyRef, target: &ReExportTarget| {
        end_on_module(
            dep,
            target,
            reexport_map,
            module_paths,
            importer_module,
            meaning,
        )
    };

    loop {
        if !visited.insert((dep.target_package.clone(), dep.target_module.clone())) {
            break;
        }
        let Some(module_info) = crate_exports.get(&dep.target_module) else {
            break;
        };
        let Some(item) = &lookup_name else {
            break;
        };

        if module_info.definitions.contains_key(item) {
            break;
        }

        // Tier 1: Explicit re-export.
        // Tier 1b: Private `use` binding, visible only to descendants of the
        // module that holds it, or of the module `pub(in …)` names, in the
        // same crate. A descendant naming the symbol through this ancestor
        // really depends on the definer, in the same crate or another
        // workspace crate.
        let binding = visible_binding(module_info, item, |visibility| {
            visible(&dep.target_package, visibility)
        });
        if let Some((target, private_use)) = binding {
            if ends_on_module(dep, target) {
                break;
            }
            if private_use {
                use_holders.push((dep.target_package.clone(), dep.target_module.clone()));
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
            let Some(next_exports) = reexport_map.get(&dep.target_package) else {
                break;
            };
            crate_exports = next_exports;
            continue;
        }

        // Tier 1c: Bound elsewhere, visible only to descendants like Tier 1b.
        // Nothing here names a definer, so the reference produces no edge.
        if module_info
            .elsewhere
            .get(item)
            .is_some_and(|visibility| visible(&dep.target_package, visibility))
        {
            return false;
        }

        // Tier 2: Glob re-exports
        // Tier 2a: Glob re-exports from a module of another workspace crate
        if let Some(next_exports) = follow_public_glob(
            dep,
            crate_exports,
            module_info,
            reexport_map,
            item,
            importer_module,
            &visited,
        ) {
            crate_exports = next_exports;
            continue;
        }

        // Tier 2b: Private glob, visible only to descendants of the module
        // that holds it, like Tier 1b.
        if let Some(next_exports) = follow_private_glob(
            dep,
            crate_exports,
            module_info,
            reexport_map,
            item,
            importer_module,
            passed_on,
        ) {
            crate_exports = next_exports;
            continue;
        }

        break;
    }
    if let (Some(passed_on), Some(name)) = (passed_on, &lookup_name) {
        passed_on.note_uses(use_holders, importer_module, dep, name);
    }
    // A rename on the way leaves the written name behind: the symbol is the
    // one the module the chain ends on knows.
    if dep.target_item.is_some() {
        dep.target_item = lookup_name;
    }
    true
}

/// The binding of `item` in `module_info` that `visible` lets through: a
/// `pub use` first, then a private `use`, marked `true`.
fn visible_binding<'a>(
    module_info: &'a ModuleExportInfo,
    item: &str,
    visible: impl Fn(&Visibility) -> bool,
) -> Option<(&'a ReExportTarget, bool)> {
    let visible_target = |target: &&ReExportTarget| visible(&target.visibility);
    module_info
        .explicit_reexports
        .get(item)
        .filter(visible_target)
        .map(|target| (target, false))
        .or_else(|| {
            module_info
                .private_uses
                .get(item)
                .filter(visible_target)
                .map(|target| (target, true))
        })
}

/// Point `dep` at the module a glob re-export of `module_info` imports from
/// that exports `symbol` to `importer_module` without passing a module in
/// `passed`: one of this crate first, then one of another workspace crate.
/// Returns the export info of that module's crate, where the chain goes on.
fn follow_public_glob<'m>(
    dep: &mut DependencyRef,
    crate_exports: &'m HashMap<String, ModuleExportInfo>,
    module_info: &ModuleExportInfo,
    reexport_map: &'m ReExportMap,
    symbol: &str,
    importer_module: &str,
    passed: &HashSet<(String, String)>,
) -> Option<&'m HashMap<String, ModuleExportInfo>> {
    if follow_local_glob(
        dep,
        &module_info.glob_sources,
        reexport_map,
        symbol,
        importer_module,
        passed,
    ) {
        return Some(crate_exports);
    }
    follow_foreign_glob(
        dep,
        &module_info.foreign_glob_sources,
        reexport_map,
        symbol,
        importer_module,
        passed,
    )
}

/// Point `dep` at the module a private glob of `module_info` imports from
/// that brings `symbol` into the scope of `importer_module`: one of this
/// crate, noted in `passed_on`, or else one of another workspace crate.
/// Returns the export info of that module's crate, where the chain goes on.
fn follow_private_glob<'m>(
    dep: &mut DependencyRef,
    crate_exports: &'m HashMap<String, ModuleExportInfo>,
    module_info: &ModuleExportInfo,
    reexport_map: &'m ReExportMap,
    symbol: &str,
    importer_module: &str,
    passed_on: Option<&PassedOn>,
) -> Option<&'m HashMap<String, ModuleExportInfo>> {
    if let Some(glob_src) = private_glob_holding(
        reexport_map,
        &dep.target_package,
        module_info,
        symbol,
        importer_module,
    ) {
        if let Some(passed_on) = passed_on.filter(|_| importer_module != dep.target_module) {
            passed_on.note_glob(dep, glob_src);
        }
        dep.target_module.clone_from(glob_src);
        return Some(crate_exports);
    }
    follow_foreign_glob(
        dep,
        &module_info.foreign_private_glob_sources,
        reexport_map,
        symbol,
        importer_module,
        &HashSet::new(),
    )
}

/// The module a private glob of `module_info` imports from that brings
/// `symbol` into the scope of `importer_module`.
fn private_glob_holding<'m>(
    reexport_map: &ReExportMap,
    crate_name: &str,
    module_info: &'m ModuleExportInfo,
    symbol: &str,
    importer_module: &str,
) -> Option<&'m String> {
    module_info
        .private_glob_sources
        .iter()
        .filter(|glob| {
            reexport_map.is_visible(crate_name, false, &glob.visibility, importer_module)
        })
        .map(|glob| &glob.module)
        .find(|glob_src| {
            module_scope_holds_symbol(
                reexport_map,
                crate_name,
                glob_src,
                symbol,
                importer_module,
                &mut HashSet::new(),
            )
        })
}

/// Point `dep` at the first of the glob sources `sources`, modules of the
/// crate `dep` points into, that exports `symbol` to `importer_module`
/// without passing a module in `passed`: a source that reaches it only
/// through a module the chain already passed leads back, not on. A glob
/// `importer_module` does not see brings nothing in. Returns whether one
/// does.
fn follow_local_glob(
    dep: &mut DependencyRef,
    sources: &[GlobSource],
    reexport_map: &ReExportMap,
    symbol: &str,
    importer_module: &str,
    passed: &HashSet<(String, String)>,
) -> bool {
    let Some(glob_src) = sources
        .iter()
        .filter(|glob| {
            reexport_map.is_visible(
                &dep.target_package,
                false,
                &glob.visibility,
                importer_module,
            )
        })
        .map(|glob| &glob.module)
        .find(|glob_src| {
            module_exports_symbol(
                reexport_map,
                &dep.target_package,
                glob_src,
                symbol,
                importer_module,
                &mut passed.clone(),
            )
        })
    else {
        return false;
    };
    tracing::debug!(
        "re-export resolved: {} -> {} (via glob re-export in {})",
        dep.target_module,
        glob_src,
        dep.target_module
    );
    dep.target_module.clone_from(glob_src);
    true
}

/// Point `dep` at the first of the glob sources `sources`, modules of another
/// workspace crate by crate name and module path, held by a module of the
/// crate `dep` points into, that exports `symbol` to `importer_module`
/// without passing a module in `passed`. A glob `importer_module` does not
/// see brings nothing in. Returns that crate's export info, where the chain
/// goes on.
fn follow_foreign_glob<'m>(
    dep: &mut DependencyRef,
    sources: &[(String, GlobSource)],
    reexport_map: &'m ReExportMap,
    symbol: &str,
    importer_module: &str,
    passed: &HashSet<(String, String)>,
) -> Option<&'m HashMap<String, ModuleExportInfo>> {
    sources.iter().find_map(|(crate_name, glob)| {
        let exports = reexport_map.get(crate_name)?;
        if !reexport_map.is_visible(
            &dep.target_package,
            false,
            &glob.visibility,
            importer_module,
        ) || !module_exports_symbol(
            reexport_map,
            crate_name,
            &glob.module,
            symbol,
            importer_module,
            &mut passed.clone(),
        ) {
            return None;
        }
        dep.target_package.clone_from(crate_name);
        dep.target_module.clone_from(&glob.module);
        Some(exports)
    })
}

/// Point `dep` at the module `target` binds, when it binds a module or a crate
/// root that `importer_module` sees: the reference then names the module, not
/// an item in it. A reference whose `meaning` is no module goes on to an item
/// of that name beside the module, where its holder shows one.
fn end_on_module(
    dep: &mut DependencyRef,
    target: &ReExportTarget,
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
    importer_module: &str,
    meaning: Meaning,
) -> bool {
    let item_meant = || {
        !meaning.admits_a_module()
            && holds_an_item_named(
                reexport_map,
                module_paths,
                &target.crate_name,
                dep.in_lib_view,
                &target.module,
                &target.original_name,
                importer_module,
            )
    };
    let binds_module = binds_a_module(target, module_paths)
        && module_is_visible(reexport_map, target, dep.in_lib_view, importer_module)
        && !item_meant();
    if binds_module {
        dep.target_package.clone_from(&target.crate_name);
        dep.target_module = append_to_path(&target.module, &target.original_name);
        dep.target_item = None;
    }
    binds_module
}

/// Whether `module` of `crate_name`, reached in the lib view or not, holds an
/// item `name` visible from `importer_module`: its own definition, or a `use`
/// of something that is no module. Beside a module of that name, it is what a
/// reference that does not mean the module names.
fn holds_an_item_named(
    reexport_map: &ReExportMap,
    module_paths: &ModulePathMap,
    crate_name: &str,
    in_lib_view: bool,
    module: &str,
    name: &str,
    importer_module: &str,
) -> bool {
    let Some(info) = reexport_map
        .modules_of(crate_name, in_lib_view)
        .and_then(|modules| modules.get(module))
    else {
        return false;
    };
    let visible = |visibility: &Visibility| {
        reexport_map.is_visible(crate_name, in_lib_view, visibility, importer_module)
    };
    let defined =
        info.definitions.contains_key(name) && info.item_visibility.get(name).is_none_or(visible);
    defined
        || visible_binding(info, name, visible)
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

/// Whether the module `target` binds is visible from `module`, as the `mod`
/// declaration in its parent has it. A crate root is, and so is a module the
/// map holds no restricted declaration of.
fn module_is_visible(
    reexport_map: &ReExportMap,
    target: &ReExportTarget,
    in_lib_view: bool,
    module: &str,
) -> bool {
    reexport_map
        .modules_of(&target.crate_name, in_lib_view)
        .and_then(|crate_exports| crate_exports.get(&target.module))
        .and_then(|parent| parent.module_visibility.get(&target.original_name))
        .is_none_or(|visibility| {
            reexport_map.is_visible(&target.crate_name, in_lib_view, visibility, module)
        })
}

/// Check whether a module of `crate_name` exports a symbol to
/// `importer_module` (own definition OR re-export), following glob re-exports
/// into other workspace crates. What `importer_module` does not see, it does
/// not export there. A module in `visited`, by crate and module path, exports
/// nothing more.
fn module_exports_symbol(
    reexport_map: &ReExportMap,
    crate_name: &str,
    module_path: &str,
    symbol: &str,
    importer_module: &str,
    visited: &mut HashSet<(String, String)>,
) -> bool {
    if !visited.insert((crate_name.to_string(), module_path.to_string())) {
        return false;
    }
    let Some(info) = reexport_map
        .get(crate_name)
        .and_then(|crate_exports| crate_exports.get(module_path))
    else {
        return false;
    };
    let visible = |visibility: &Visibility| {
        reexport_map.is_visible(crate_name, false, visibility, importer_module)
    };
    let defined = info.definitions.contains_key(symbol)
        && info.item_visibility.get(symbol).is_none_or(visible);
    if defined {
        return true;
    }
    if info
        .explicit_reexports
        .get(symbol)
        .is_some_and(|target| visible(&target.visibility))
    {
        return true;
    }
    let local_globs = info.glob_sources.iter().map(|glob| (crate_name, glob));
    let foreign_globs = info
        .foreign_glob_sources
        .iter()
        .map(|(glob_crate, glob)| (glob_crate.as_str(), glob));
    local_globs
        .chain(foreign_globs)
        .filter(|(_, glob)| visible(&glob.visibility))
        .any(|(glob_crate, glob)| {
            module_exports_symbol(
                reexport_map,
                glob_crate,
                &glob.module,
                symbol,
                importer_module,
                visited,
            )
        })
}

/// Whether a `pub use` in the root of `crate_name` brings `symbol` into the
/// root, by name or through a glob that `importer_module` sees. A glob does
/// not name the symbol in the root file, so only the map knows it.
fn root_reexports(
    reexport_map: &ReExportMap,
    crate_name: &str,
    symbol: &str,
    importer_module: &str,
) -> bool {
    let Some(crate_exports) = reexport_map.get(crate_name) else {
        return false;
    };
    crate_exports.get("").is_some_and(|root| {
        root.explicit_reexports.contains_key(symbol)
            || root
                .glob_sources
                .iter()
                .filter(|glob| {
                    reexport_map.is_visible(crate_name, false, &glob.visibility, importer_module)
                })
                .any(|glob| {
                    module_exports_symbol(
                        reexport_map,
                        crate_name,
                        &glob.module,
                        symbol,
                        importer_module,
                        &mut HashSet::new(),
                    )
                })
    })
}

/// Whether the root of `crate_name` defines `symbol` itself, privately or not.
fn root_defines(reexport_map: &ReExportMap, crate_name: &str, symbol: &str) -> bool {
    reexport_map
        .get(crate_name)
        .and_then(|crate_exports| crate_exports.get(""))
        .is_some_and(|root| {
            root.definitions.contains_key(symbol) || root.item_visibility.contains_key(symbol)
        })
}

/// The module a `pub use` in the root of `crate_name` brings in as `name`, by
/// name or through a glob: `pub use errors::*` with `pub mod shell_error` in
/// `errors` makes `shell_error` in the root name `errors::shell_error`.
fn root_reexported_module(
    reexport_map: &ReExportMap,
    crate_name: &str,
    name: &str,
    module_paths: &HashSet<String>,
) -> Option<String> {
    let crate_exports = reexport_map.get(crate_name)?;
    let root = crate_exports.get("")?;
    if let Some(target) = root.explicit_reexports.get(name) {
        let module = append_to_path(&target.module, &target.original_name);
        return (target.crate_name == crate_name && module_paths.contains(&module))
            .then_some(module);
    }
    root.glob_sources.iter().find_map(|glob| {
        glob_child_module(
            crate_exports,
            &glob.module,
            name,
            module_paths,
            &mut HashSet::new(),
        )
    })
}

/// The crate and module in another workspace crate that a `pub use` in the
/// root of `crate_name` brings in as `name`, following re-exports from root to
/// root: `pub use other::m;` names module `m` of `other`, `pub use other;` its
/// root (module `""`).
fn root_reexported_foreign_module(
    ctx: &ResolutionContext,
    reexport_map: &ReExportMap,
    crate_name: &str,
    name: &str,
) -> Option<(String, String)> {
    let mut visited = HashSet::new();
    let (mut current_crate, mut current_name) = (crate_name.to_string(), name.to_string());
    loop {
        if !visited.insert((current_crate.clone(), current_name.clone())) {
            return None;
        }
        let target = reexport_map
            .get(&current_crate)?
            .get("")?
            .explicit_reexports
            .get(&current_name)?;
        let module = append_to_path(&target.module, &target.original_name);
        let names_module = module.is_empty()
            || ctx
                .all_module_paths
                .get_or_empty(&target.crate_name)
                .contains(&module);
        if names_module {
            return (target.crate_name != crate_name).then(|| (target.crate_name.clone(), module));
        }
        if !target.module.is_empty() {
            return None;
        }
        current_crate = target.crate_name.clone();
        current_name = target.original_name.clone();
    }
}

/// `parts`, a path inside `crate_name`, rewritten as a path that starts at
/// another workspace crate, when the root of `crate_name` re-exports that
/// crate or one of its modules under the first segment: beside
/// `pub use other;` the path `other::error::E` runs on in `other`.
fn path_in_root_reexported_crate(
    ctx: &ResolutionContext,
    reexport_map: &ReExportMap,
    crate_name: &str,
    parts: &[&str],
) -> Option<String> {
    let (first, rest) = parts.split_first()?;
    let first = first.trim_end_matches('{').trim();
    let (other_crate, module) =
        root_reexported_foreign_module(ctx, reexport_map, crate_name, first)?;
    Some(
        std::iter::once(other_crate.as_str())
            .chain((!module.is_empty()).then_some(module.as_str()))
            .chain(rest.iter().copied())
            .collect::<Vec<_>>()
            .join("::"),
    )
}

/// The child module `name` of `module_path`, or of a module its globs
/// republish.
fn glob_child_module(
    crate_exports: &HashMap<String, ModuleExportInfo>,
    module_path: &str,
    name: &str,
    module_paths: &HashSet<String>,
    visited: &mut HashSet<String>,
) -> Option<String> {
    if !visited.insert(module_path.to_string()) {
        return None;
    }
    let child = append_to_path(module_path, name);
    if module_paths.contains(&child) {
        return Some(child);
    }
    crate_exports
        .get(module_path)?
        .glob_sources
        .iter()
        .find_map(|glob| {
            glob_child_module(crate_exports, &glob.module, name, module_paths, visited)
        })
}

/// Whether `importer_module` can name `symbol` through `module_path`: what
/// the module exports, plus what its private globs forward to a module that
/// sees them. A private glob is followed on direct hops only; a `pub use
/// m::*` on the way republishes `m`'s public items, none of `m`'s private
/// imports.
fn module_scope_holds_symbol(
    reexport_map: &ReExportMap,
    crate_name: &str,
    module_path: &str,
    symbol: &str,
    importer_module: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(module_path.to_string()) {
        return false;
    }
    if module_exports_symbol(
        reexport_map,
        crate_name,
        module_path,
        symbol,
        importer_module,
        &mut HashSet::new(),
    ) {
        return true;
    }
    let Some(info) = reexport_map
        .get(crate_name)
        .and_then(|crate_exports| crate_exports.get(module_path))
    else {
        return false;
    };
    info.private_glob_sources
        .iter()
        .filter(|glob| {
            reexport_map.is_visible(crate_name, false, &glob.visibility, importer_module)
        })
        .any(|glob| {
            module_scope_holds_symbol(
                reexport_map,
                crate_name,
                &glob.module,
                symbol,
                importer_module,
                visited,
            )
        })
}

/// Whether `module` is `ancestor` itself or nested below it. The crate root
/// (`""`) is an ancestor of every module.
fn is_descendant(module: &str, ancestor: &str) -> bool {
    ancestor.is_empty() || module == ancestor || module.starts_with(&format!("{ancestor}::"))
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
    /// Where to note the private globs and private `use` items of ancestors
    /// this file names something through. `None` where no caller asks.
    pub(crate) passed_on: Option<&'a PassedOn>,
}

impl ResolutionContext<'_> {
    /// [`resolve_reexport_noting`] from this file's module, for a reference
    /// that asks `meaning` of `dep`'s item.
    fn resolve_reexport(&self, dep: &mut DependencyRef, meaning: Meaning) -> bool {
        resolve_reexport_noting(
            dep,
            self.reexport_map,
            self.all_module_paths,
            self.current_module_path,
            self.passed_on,
            meaning,
        )
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
fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
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
}

/// One path reference with what its position says about it.
#[derive(Debug)]
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
    /// Resolve `path`, this reference or the path a binding rebased it to,
    /// through the resolution chain. A path written in macro input resolves
    /// through a binding only.
    fn resolve(&self, ctx: &ResolutionContext, path: &str) -> Option<DependencyRef> {
        if self.in_macro {
            return None;
        }
        resolve_single_path(
            ctx,
            path,
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
/// after the longest module prefix that this module binds to another module
/// of the crate, by `pub use` or, for a descendant of the module, by a
/// private `use`, carries the rest of the path into that module: beside
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
            .and_then(|name| module_bound_as(ctx, crate_name, &module, name, module_paths));
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

/// The module of `crate_name` that `module` binds as `name`, where the file
/// being resolved can see the binding.
fn module_bound_as(
    ctx: &ResolutionContext,
    crate_name: &str,
    module: &str,
    name: &str,
    module_paths: &HashSet<String>,
) -> Option<String> {
    let info = ctx.reexport_map.get(crate_name)?.get(module)?;
    let visible = |target: &&ReExportTarget| {
        ctx.reexport_map.is_visible(
            crate_name,
            false,
            &target.visibility,
            ctx.current_module_path,
        )
    };
    let target = info
        .explicit_reexports
        .get(name)
        .filter(visible)
        .or_else(|| info.private_uses.get(name).filter(visible))?;
    let bound = append_to_path(&target.module, &target.original_name);
    (target.crate_name == crate_name && module_paths.contains(&bound)).then_some(bound)
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
    if parts.len() > 1
        && !module_paths.contains(first)
        && let Some(in_other) =
            path_in_root_reexported_crate(ctx, ctx.reexport_map, &crate_name, &parts)
    {
        return parse_workspace_import(ctx, &in_other, line_num, context);
    }
    let root_module = if module_paths.contains(first) {
        None
    } else {
        root_reexported_module(ctx.reexport_map, &crate_name, first, module_paths)
    };
    let (target_module, target_item) = if let Some(module) = &root_module {
        let in_crate: Vec<&str> = module
            .split("::")
            .chain(parts[1..].iter().copied())
            .collect();
        module_and_item(ctx, &crate_name, &in_crate, module_paths)
    } else if !module_paths.contains(first)
        && (root_reexports(
            ctx.reexport_map,
            &crate_name,
            first,
            ctx.current_module_path,
        ) || root_defines(ctx.reexport_map, &crate_name, first))
    {
        (String::new(), Some(first.to_string()))
    } else {
        module_and_item(ctx, &crate_name, &parts, module_paths)
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
        in_lib_view: false,
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
        in_lib_view: false,
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
    // The own package by name is its lib, also from a file of the bin tree.
    let names_own_package = target_crate_name == normalize_crate_name(ctx.current_crate);
    let lib_view;
    let reexport_map = if names_own_package {
        lib_view = ctx.reexport_map.lib_view();
        &lib_view
    } else {
        ctx.reexport_map
    };
    let module_paths = ctx.all_module_paths.get_or_empty(&target_crate_name);
    if parts.len() > 2
        && !module_paths.contains(module_segment)
        && let Some(in_other) =
            path_in_root_reexported_crate(ctx, reexport_map, &target_crate_name, &parts[1..])
    {
        return parse_workspace_import(ctx, &in_other, line_num, context);
    }
    let root_module = if module_paths.contains(module_segment) {
        None
    } else {
        root_reexported_module(
            reexport_map,
            &target_crate_name,
            module_segment,
            module_paths,
        )
    };
    let in_crate: Vec<&str> = match &root_module {
        Some(module) => module
            .split("::")
            .chain(parts[2..].iter().copied())
            .collect(),
        None => parts[1..].to_vec(),
    };
    let (target_module, prefix_len) = find_longest_module_prefix(&in_crate, module_paths);

    // Entry-point detection: if the resolved target_module is not a known module
    // and the first segment after the crate name is a known export, treat it as
    // an entry-point dependency (target_module = ""). `crate_exports` reads only
    // the root file, so a name a root glob brings in is looked up in the map.
    let is_entry_point = !module_paths.contains(&target_module)
        && (ctx
            .crate_exports
            .get(&target_crate_name)
            .is_some_and(|e| e.contains(module_segment))
            || root_reexports(
                reexport_map,
                &target_crate_name,
                module_segment,
                ctx.current_module_path,
            ));

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
        in_lib_view: names_own_package,
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
    if !meaning.admits_a_module()
        && holds_an_item_named(
            ctx.reexport_map,
            ctx.all_module_paths,
            &dep.target_package,
            dep.in_lib_view,
            parent,
            name,
            ctx.current_module_path,
        )
    {
        let (parent, name) = (parent.to_string(), name.to_string());
        dep.target_module = parent;
        dep.target_item = Some(name);
        return;
    }
    let visible = |visibility: &Visibility| {
        ctx.reexport_map.is_visible(
            &dep.target_package,
            dep.in_lib_view,
            visibility,
            ctx.current_module_path,
        )
    };
    let Some(target) = ctx
        .reexport_map
        .exports_for(dep)
        .and_then(|modules| modules.get(parent))
        .filter(|info| {
            info.module_visibility
                .get(name)
                .is_some_and(|visibility| !visible(visibility))
        })
        .and_then(|info| info.explicit_reexports.get(name))
        .filter(|target| visible(&target.visibility))
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
                in_lib_view: false,
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
        .or_else(|| {
            // Bare workspace crate name (e.g. from `use other_crate::{Foo}` → path = "other_crate")
            let path = in_workspace;
            if !path.contains("::") && ctx.workspace_crates.contains(path) {
                Some(DependencyRef {
                    target_package: path.to_string(),
                    target_module: String::new(),
                    target_item: None,
                    source_file: ctx.source_file.to_path_buf(),
                    line: line_num,
                    context: context.clone(),
                    via_reexport: false,
                    uses: Vec::new(),
                    in_lib_view: normalize_crate_name(path)
                        == normalize_crate_name(ctx.current_crate),
                })
            } else {
                None
            }
        })
        .or_else(|| parse_external_crate_import(ctx, path, line_num, context))
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
        in_lib_view: false,
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
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashMap<(String, UsageKind), usize> = HashMap::new();

    for collected in use_items.iter() {
        let item = &collected.item;
        let line_num = item.use_token.span.start().line;
        let paths = resolve_use_tree(&item.tree, "", false)
            .into_iter()
            .map(|path| bindings.rebased(&path, collected.region));
        // A `use` that republishes a name under a visibility reachable from
        // outside the module is a re-export. Path references (non-use) are never
        // re-exports.
        let via_reexport = is_reexport_visibility(&item.vis);

        // A `use` binds every meaning of its target.
        for path in paths {
            if let Some(mut dep) = resolve_single_path(
                ctx,
                &path,
                line_num,
                &collected.context,
                collected.inline_depth,
                Meaning::Any,
            ) {
                // A private glob whose target the map knows is resolved into
                // the names the file uses, by `parse_path_ref_dependencies`.
                // A `pub use` glob republishes every name and uses none, so it
                // is the whole payload.
                if is_glob(&dep) && glob_payload_known(&dep, ctx.reexport_map) && !via_reexport {
                    continue;
                }
                dep.via_reexport = via_reexport;
                for mut dep in expand_glob(dep, ctx.reexport_map) {
                    if ctx.resolve_reexport(&mut dep, Meaning::Any) {
                        DependencyRef::dedup_push(&mut deps, &mut seen_targets, dep);
                    }
                }
            }
        }
    }

    deps
}

/// The names `use <module>::*` brings into scope: the module's own definitions
/// plus what it republishes under a name.
///
/// `None` when the module is absent from the map or exports nothing under a name
/// it can see. Sorted, because the map iterates in arbitrary order and the names
/// reach the rendered output.
fn glob_payload(dep: &DependencyRef, reexport_map: &ReExportMap) -> Option<Vec<String>> {
    let module_info = reexport_map.module_info(dep)?;
    let mut names: Vec<String> = module_info
        .definitions
        .keys()
        .chain(module_info.explicit_reexports.keys())
        .cloned()
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
fn expand_glob(dep: DependencyRef, reexport_map: &ReExportMap) -> Vec<DependencyRef> {
    if !is_glob(&dep) {
        return vec![dep];
    }
    let Some(names) = glob_payload(&dep, reexport_map) else {
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
/// build `glob_sources`, which is what `resolve_reexport` resolves glob chains
/// against.
fn is_glob(dep: &DependencyRef) -> bool {
    dep.target_item.as_deref() == Some("*")
}

/// Whether the map holds the module a glob imports from, and so can say which
/// names the glob brings in.
fn glob_payload_known(dep: &DependencyRef, reexport_map: &ReExportMap) -> bool {
    reexport_map.module_info(dep).is_some()
}

/// What every name in one file means, per region: module aliases, names bound
/// elsewhere, names bound to items, glob imports, and the file's own item
/// names.
///
/// `modules` holds a binding name → the absolute path of the module it names.
/// Absolute means `crate::a::b` for the current crate and `other_crate::b`
/// otherwise, so a binding stays valid wherever in the file it is used.
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
/// `glob_modules` holds, by name and as absolute paths like `modules`, the
/// child modules of a glob's module and the names that module binds to a
/// module by `pub use` or, where the file sits below it in the same crate, by
/// a private `use`. A glob brings them in like any other name, so a path may
/// start with one.
#[derive(Debug, Default)]
pub(crate) struct FileBindings {
    regions: BindingRegions,
    modules: HashMap<RegionId, HashMap<String, String>>,
    elsewhere: HashMap<RegionId, HashSet<String>>,
    items: HashMap<RegionId, HashMap<String, DependencyRef>>,
    globs: HashMap<RegionId, Vec<DependencyRef>>,
    glob_modules: HashMap<RegionId, HashMap<String, String>>,
    /// The names of the items the file defines, at any depth. A glob never
    /// brings in a name the file defines itself.
    local_definitions: HashSet<String>,
}

/// What one path reference stands for, given the bindings around it.
enum PathBinding<'a> {
    /// A path to run through the resolution chain, with a bound module alias
    /// replaced by the module's absolute path.
    Path(Cow<'a, str>),
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
            if let Binding::Module(module_path) = classify_binding(ctx, chained.as_ref(), &binding)
            {
                bindings.bind_module(collected.region, &binding, module_path);
            }
        }

        for (collected, binding, original_path) in use_bindings(use_items) {
            let binding = binding.as_str();
            let path = bindings.rebased(&original_path, collected.region);
            let dep = resolve_single_path(
                ctx,
                &path,
                collected.item.use_token.span.start().line,
                &collected.context,
                collected.inline_depth,
                Meaning::Any,
            );
            // A glob binds every name its module exports and none of them is
            // written here; the names are found where the file uses them.
            if binding == "*" {
                if let Some(mut dep) = dep
                    && glob_payload_known(&dep, ctx.reexport_map)
                {
                    dep.via_reexport = is_reexport_visibility(&collected.item.vis);
                    bindings.bind_glob(collected.region, dep);
                }
                continue;
            }
            match classify_binding(ctx, through_the_chain(ctx, dep.as_ref()).as_ref(), binding) {
                Binding::Module(module_path) => {
                    bindings.bind_module(collected.region, binding, module_path);
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

    /// The module path a name is bound to as seen from `region`, where it is
    /// bound to one.
    #[cfg(test)]
    fn target(&self, region: RegionId, name: &str) -> Option<&str> {
        self.regions
            .enclosing(region)
            .find_map(|enclosing| self.modules.get(&enclosing)?.get(name))
            .map(String::as_str)
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
            if let Some(target) = self.modules.get(&enclosing).and_then(|m| m.get(first))
                && let Some(rest) = rest
            {
                return PathBinding::Path(Cow::Owned(format!("{target}::{rest}")));
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
            if let Some(target) = self.glob_modules.get(&enclosing).and_then(|m| m.get(first))
                && let Some(rest) = rest
                && !self.local_definitions.contains(first)
            {
                return PathBinding::Path(Cow::Owned(format!("{target}::{rest}")));
            }
        }
        PathBinding::Unbound(first, last)
    }

    /// `path` with a first segment bound to a module replaced by the module's
    /// absolute path, as seen from `region`.
    fn rebased(&self, path: &str, region: RegionId) -> String {
        match self.binding_of(path, region) {
            PathBinding::Path(rebased) => rebased.into_owned(),
            _ => path.to_string(),
        }
    }

    /// List the glob imports a name standing in `region` may come from,
    /// innermost first. Empty when the file defines the name itself.
    fn globs_for(&self, name: &str, region: RegionId) -> impl Iterator<Item = &DependencyRef> {
        let defined_here = self.local_definitions.contains(name);
        self.regions
            .enclosing(region)
            .filter(move |_| !defined_here)
            .filter_map(|enclosing| self.globs.get(&enclosing))
            .flatten()
    }

    fn bind_module(&mut self, region: RegionId, name: &str, module_path: String) {
        self.modules
            .entry(region)
            .or_default()
            .insert(name.to_string(), module_path);
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
        let visible = |visibility: &Visibility| {
            ctx.reexport_map.is_visible(
                &dep.target_package,
                dep.in_lib_view,
                visibility,
                ctx.current_module_path,
            )
        };
        let info = ctx.reexport_map.module_info(dep);
        let module_paths = ctx.all_module_paths.get_or_empty(&dep.target_package);
        let children = module_paths.iter().filter_map(|path| {
            let (parent, name) = path.rsplit_once("::").unwrap_or(("", path));
            let seen = info
                .and_then(|info| info.module_visibility.get(name))
                .is_none_or(visible);
            if parent != dep.target_module || !seen {
                return None;
            }
            let child = DependencyRef {
                target_module: path.clone(),
                target_item: None,
                ..dep.clone()
            };
            Some((name.to_string(), absolute_module_path(ctx, &child)?))
        });
        let used_modules = info
            .into_iter()
            .flat_map(|info| info.explicit_reexports.iter().chain(&info.private_uses))
            .filter(|(_, target)| {
                visible(&target.visibility)
                    && module_is_visible(
                        ctx.reexport_map,
                        target,
                        dep.in_lib_view,
                        ctx.current_module_path,
                    )
            })
            .filter_map(|(name, target)| {
                let module = append_to_path(&target.module, &target.original_name);
                let bound = DependencyRef {
                    target_package: target.crate_name.clone(),
                    target_module: module,
                    target_item: None,
                    ..dep.clone()
                };
                if !ctx
                    .all_module_paths
                    .get_or_empty(&bound.target_package)
                    .contains(&bound.target_module)
                {
                    return None;
                }
                Some((name.clone(), absolute_module_path(ctx, &bound)?))
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
    /// Names a module. A later `binding::Item` resolves through this path.
    Module(String),
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
/// what follows the name resolves under that path.
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
        return absolute_module_path(ctx, dep).map_or(Binding::Elsewhere, Binding::Module);
    }
    if dep.target_package == normalize_crate_name(ctx.current_crate) {
        return Binding::OwnItem;
    }
    if parse_bare_module_import(ctx, binding, 0, &EdgeContext::production()).is_some() {
        return spelled_in_this_crate(ctx, dep).map_or(Binding::Elsewhere, Binding::Module);
    }
    Binding::Elsewhere
}

/// `dep` followed through the re-export chain, without noting what the chain
/// passes on, so that a `use` of a name an ancestor binds to a module is
/// classified as the module. Where the chain leads out of the crate, `dep`
/// as it resolved.
fn through_the_chain(
    ctx: &ResolutionContext,
    dep: Option<&DependencyRef>,
) -> Option<DependencyRef> {
    let dep = dep?;
    let mut chained = dep.clone();
    let unnoted = ResolutionContext {
        passed_on: None,
        ..*ctx
    };
    Some(if unnoted.resolve_reexport(&mut chained, Meaning::Any) {
        chained
    } else {
        dep.clone()
    })
}

/// Render a resolved module dependency as a path that `resolve_single_path` can
/// re-resolve from any position in the file. Another crate's root is its name.
/// `None` where no path can, as [`spelled_in_this_crate`] decides.
fn absolute_module_path(ctx: &ResolutionContext, dep: &DependencyRef) -> Option<String> {
    if dep.target_package == normalize_crate_name(ctx.current_crate) {
        Some(format!("crate::{}", dep.target_module))
    } else {
        spelled_in_this_crate(ctx, dep)
    }
}

/// `dep.full_target()` with the crate written as this crate's code names it.
/// `extern_crates` turns a name into a crate before the crate keys are asked,
/// so a key it holds for another crate cannot stand for its own crate; then
/// the crate is written under a name `extern_crates` gives it, and `None`
/// stands where it gives none.
fn spelled_in_this_crate(ctx: &ResolutionContext, dep: &DependencyRef) -> Option<String> {
    let key = dep.target_package.as_str();
    let target = dep.full_target();
    if ctx.extern_crates.get(key).is_none_or(|named| named == key) {
        return Some(target);
    }
    let (name, _) = ctx
        .extern_crates
        .iter()
        .find(|(_, named)| named.as_str() == key)?;
    Some(format!("{name}{}", &target[key.len()..]))
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
pub(crate) fn parse_path_ref_dependencies(
    paths: &CollectedPathRefs,
    ctx: &ResolutionContext,
    bindings: &FileBindings,
) -> Vec<DependencyRef> {
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
        let resolve = |path: &str| path_ref.resolve(ctx, path);
        let (mut dep, associated, glob_taken_from) =
            match bindings.binding_of(&path_ref.path, path_ref.region) {
                PathBinding::Unbound(name, associated) => {
                    // A qualified path resolves on its own where it can; what
                    // does not, and a bare name, is a glob's where a glob
                    // exports it.
                    if let Some(dep) = associated.and_then(|_| resolve(&path_ref.path)) {
                        let associated = associated_segment(&path_ref.path, &dep);
                        (dep, associated, None)
                    } else {
                        let Some(glob) = bindings
                            .globs_for(name, path_ref.region)
                            .find(|glob| glob_exports(ctx, glob, name))
                        else {
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
                    if let Some(dep) = associated.and_then(|_| resolve(&path_ref.path)) {
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
                PathBinding::Path(effective) => {
                    let Some(dep) = resolve(&effective) else {
                        continue;
                    };
                    let associated = associated_segment(&path_ref.path, &dep);
                    (dep, associated, None)
                }
                PathBinding::Elsewhere => continue,
            };
        let meaning = path_ref.meaning_of_item(associated);
        if path_ref.position == PathPosition::Binding && declares_a_variable(ctx, &dep, meaning) {
            continue;
        }
        if !ctx.resolve_reexport(&mut dep, meaning) {
            continue;
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
            category: categorize(ctx, &dep, item, path_ref.position, associated),
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

    deps
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

/// Whether the module a glob imports from brings `name` into this file's
/// scope: by its own definition, a named re-export, a glob re-export chain,
/// or a private glob the file's module sees as a descendant.
fn glob_exports(ctx: &ResolutionContext, glob: &DependencyRef, name: &str) -> bool {
    let lib_view;
    let reexport_map = if glob.in_lib_view {
        lib_view = ctx.reexport_map.lib_view();
        &lib_view
    } else {
        ctx.reexport_map
    };
    module_scope_holds_symbol(
        reexport_map,
        &glob.target_package,
        &glob.target_module,
        name,
        ctx.current_module_path,
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

fn definition_kind(ctx: &ResolutionContext, dep: &DependencyRef, item: &str) -> Option<DefKind> {
    ctx.reexport_map
        .module_info(dep)
        .and_then(|info| info.definitions.get(item))
        .map(|definition| definition.kind)
}

/// Whether a bare identifier pattern naming `dep` declares a variable, which
/// takes no name through an ancestor. The chain is resolved without noting
/// what it passes on, so only a pattern that matches an item notes it. The
/// pattern asks `meaning` of the name.
fn declares_a_variable(ctx: &ResolutionContext, dep: &DependencyRef, meaning: Meaning) -> bool {
    let unnoted = ResolutionContext {
        passed_on: None,
        ..*ctx
    };
    let mut probed = dep.clone();
    unnoted.resolve_reexport(&mut probed, meaning)
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
        definition_kind(ctx, dep, item),
        Some(DefKind::Const | DefKind::Static | DefKind::Struct)
    )
}

/// Categorize one use by what its position takes from the item and, where the
/// position leaves that open, by what the item is.
fn categorize(
    ctx: &ResolutionContext,
    dep: &DependencyRef,
    item: &str,
    position: PathPosition,
    associated: Option<&str>,
) -> UseCategory {
    let kind = definition_kind(ctx, dep, item);
    let items = ctx
        .reexport_map
        .module_info(dep)
        .and_then(|info| info.associated.get(item));
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
/// `use` items first so a location stays at the import line.
pub(crate) fn parse_file_dependencies(
    syntax: &syn::File,
    ctx: &ResolutionContext,
    base_context: EdgeContext,
) -> Vec<DependencyRef> {
    let use_items = collect_all_use_items(syntax, base_context.clone());
    let bindings = FileBindings::of(syntax, &use_items, ctx);
    let use_deps = parse_workspace_dependencies(&use_items, ctx, &bindings);

    let path_refs = collect_all_path_refs(syntax, base_context);
    let path_deps = parse_path_ref_dependencies(&path_refs, ctx, &bindings);

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
    let is_trait = definition_kind(ctx, dep, item) == Some(DefKind::Trait);
    if is_trait {
        SymbolUse {
            line: dep.line,
            category: UseCategory::Fns,
            imported_for_methods: true,
        }
    } else {
        SymbolUse {
            line: dep.line,
            category: categorize(ctx, dep, item, PathPosition::Value, None),
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
    parse_file_dependencies(&syntax, ctx, EdgeContext::production())
}

#[cfg(test)]
mod tests;
