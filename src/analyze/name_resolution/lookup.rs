//! What a name stands for in a module, as a viewer of the binding table sees
//! it: the lookup through `use` items and globs, what a glob or `use` passes
//! on, and the settling of paths the table still holds as written.

use super::super::packages::ModulePathMap;
use super::bindings::{
    BindingKind, BindingPlace, Brought, CrateModule, DefinedAt, GlobBinding, GlobPlace, GlobReach,
    Holder, Meaning, ModuleKey, NameBinding, NameTarget, Namespace, ReExportMap, ReExportTarget,
    SeenBindings, Settled, Viewer,
};
use super::place::{AfterName, append_to_path};
use crate::model::{DefKind, DependencyRef, UsageKind};
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

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
    lookup.viewer.bindings_in(holder).is_some_and(|bindings| {
        binds_to_differing_targets(bindings, name, |target| {
            !lookup.defines_only_outside_the_type_namespace(target)
        })
    })
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
    /// [`super::place::UseScope::place`] left it, at what that path
    /// reaches, and each glob whose module is written at the module it reaches.
    /// Each path is read from the root of its crate: into another workspace
    /// crate as seen from outside, in the own crate as the module holding the
    /// `use` sees it. A path that runs through a binding or glob still written
    /// waits for a later round, and rounds repeat while one settles a path. A
    /// path whose first name the root binds nothing under names that name in
    /// the root. A path from a name in scope
    /// ([`super::place::UsePlace::Name`]) whose first name stands
    /// for no module in the holding module names an external crate: a private
    /// `use` of it binds [`BindingKind::Elsewhere`], a
    /// `pub use` [`BindingKind::ReExportFromOutside`]. A glob whose path does not reach a module
    /// imports nothing.
    pub(super) fn settle_written_paths(mut self, module_paths: &ModulePathMap) -> Self {
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
                let own = target.crate_name == place.module.root.0;
                let lookup = self.lookup_of_written(own, &place.module, module_paths);
                let from = if place.from_scope {
                    WrittenFrom::Scope(&place.module.path)
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
                            .strip_prefix(&format!("{}::", place.module.path))
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
                let own = glob.crate_name == place.module.root.0;
                let lookup = self.lookup_of_written(own, &place.module, module_paths);
                let reached = reached_module(lookup, glob, unsettled)?;
                Some((place, reached))
            })
            .collect()
    }

    /// The lookup that reads a path written in `module`: from that module
    /// where the path stays in the own crate (`own`), else from outside every
    /// package.
    fn lookup_of_written<'a>(
        &'a self,
        own: bool,
        module: &'a ModuleKey,
        module_paths: &'a ModulePathMap,
    ) -> NameLookup<'a> {
        let viewer = if own {
            Viewer {
                packages: &self.packages,
                own_root: Some(&module.root),
                test_build: module.test_build,
                module: Some(&module.path),
            }
        } else {
            self.viewer_from_outside()
        };
        NameLookup {
            viewer,
            module_paths,
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
            .flat_map(|(module, info)| {
                info.globs.iter().enumerate().map(move |(index, glob)| {
                    let place = GlobPlace {
                        module: module.clone(),
                        index,
                    };
                    (place, glob)
                })
            })
            .filter(move |(_, glob)| !module_paths.holds(&glob.crate_name, &glob.module))
    }
}

/// The private `use` items and globs a descendant named something through,
/// per module that holds them. Such a module passes the names on to its
/// descendants. Each note records whether production code named something
/// through it, since only that passes a name on for production code.
#[derive(Debug, Default)]
pub(super) struct PassedOn {
    /// Holder and the module its glob imports from.
    globs: HashMap<(Holder, String), FromProduction>,
    /// Holder and where the name the descendant reached is defined.
    uses: HashMap<(Holder, DefinedAt), FromProduction>,
}

/// Whether production code, not only test code, named something through a
/// note's holder.
pub(super) type FromProduction = bool;

/// What a re-export chain followed from a reference: whether it ends on a
/// definer syn can place, the name it ends on, and the bindings of other
/// modules it passed through. Following the chain notes nothing; a caller
/// that keeps the reference notes the chain in [`PassedOn`].
#[derive(Debug)]
pub(super) struct Resolution {
    ends_on_no_definer: bool,
    name: Option<String>,
    passed: Vec<PassedThrough>,
}

impl Resolution {
    /// Whether the chain reaches a name a private `use` bound to something
    /// outside the crate, or to targets syn cannot tell apart: the reference
    /// then produces no edge.
    pub(super) fn places_no_definer(&self) -> bool {
        self.ends_on_no_definer
    }
}

/// A private binding a re-export chain passed through, by the module that
/// holds it.
#[derive(Debug, PartialEq)]
enum PassedThrough {
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
    pub(super) fn note(
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
    pub(super) fn passes_on(
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
pub(super) fn covers(note: Option<&FromProduction>, dep: &DependencyRef) -> bool {
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
pub(super) struct Visited(HashSet<(String, String, String)>);

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

/// The lookup of "what is name N in module M for viewer V": a viewer and the
/// module tree. Each question walks the binding table through
/// [`NameLookup::enter`], which reads a module with the viewer that reaches
/// its crate, records it in the walk's [`Visited`] and gives only the
/// bindings the viewer sees. No question calls [`Viewer::reaching`] or
/// records a module itself.
#[derive(Debug, Clone, Copy)]
pub(super) struct NameLookup<'a> {
    pub(super) viewer: Viewer<'a>,
    pub(super) module_paths: &'a ModulePathMap,
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
    pub(super) fn follow(self, dep: &mut DependencyRef, meaning: Meaning) -> Resolution {
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
    pub(super) fn brings(self, module: CrateModule, symbol: &str) -> Option<Brought> {
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
                || binds_to_differing_targets(bindings, symbol, |_| true)
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
    pub(super) fn name(
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
    pub(super) fn module_named(
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
    pub(super) fn name_passing(
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
    pub(super) fn declared_child(
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
    pub(super) fn holds_an_item_named(self, module: CrateModule, name: &str) -> bool {
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
        self.first_definition(module, name, visited, &|module, name, definition| {
            (definition.namespace() == Some(Namespace::Type))
                .then(|| target_in(module.crate_name.to_string(), module.path.to_string(), name))
        })
    }

    /// The namespace of the definition `module` binds as `name`, by a
    /// definition of its own or through `use` items, as the viewer sees it.
    /// `None` where no definition the map knows is reached; the modules and
    /// names in `visited` bind nothing.
    pub(super) fn definition_namespace(
        self,
        module: CrateModule,
        name: &str,
        visited: &mut Visited,
    ) -> Option<Namespace> {
        self.first_definition(module, name, visited, &|_, _, definition| {
            definition.namespace()
        })
    }

    /// The first result `of_definition` gives for a definition `module`
    /// binds as `name`, by a definition of its own or through `use` items, as
    /// the viewer sees it. `of_definition` is asked with the module and name
    /// of the definition; the modules and names in `visited` bind nothing.
    fn first_definition<T>(
        self,
        module: CrateModule,
        name: &str,
        visited: &mut Visited,
        of_definition: &impl Fn(CrateModule, &str, &NameBinding) -> Option<T>,
    ) -> Option<T> {
        let (lookup, bindings) = self.enter(module, Some(name), visited)?;
        for binding in bindings?.named(name) {
            let found = match &binding.kind {
                BindingKind::Definition(_) => of_definition(module, name, binding),
                BindingKind::ReExport(target) | BindingKind::PrivateUse(target) => {
                    let holder = CrateModule {
                        crate_name: &target.crate_name,
                        path: &target.module,
                    };
                    lookup.first_definition(holder, &target.original_name, visited, of_definition)
                }
                BindingKind::ReExportFromOutside(_)
                | BindingKind::Elsewhere
                | BindingKind::Module => None,
            };
            if found.is_some() {
                return found;
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
    pub(super) fn binds_a_module(self, target: &ReExportTarget) -> bool {
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
    pub(super) fn module_is_visible(self, target: &ReExportTarget) -> bool {
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
        Some(BindingKind::PrivateUse(_)) => private_targets_differ(bindings.every(name), |_| true),
        Some(BindingKind::ReExport(_)) => false,
        _ => {
            binds_to_differing_targets(bindings, name, |_| true)
                || binds_elsewhere(bindings.named(name))
        }
    }
}

/// Whether the private `use` items among `named`, the bindings of one name,
/// bind it to different targets among those `admits` holds for:
/// alternatives under `cfg` in one file or in alternative files.
fn private_targets_differ<'a>(
    named: impl Iterator<Item = &'a NameBinding>,
    admits: impl Fn(&ReExportTarget) -> bool,
) -> bool {
    let mut targets = named
        .filter_map(|binding| match &binding.kind {
            BindingKind::PrivateUse(target) => Some(target),
            _ => None,
        })
        .filter(|target| admits(target));
    targets
        .next()
        .is_some_and(|first| targets.any(|target| target != first))
}

/// Whether private `use` items bind `name` in the module of `bindings` to
/// different targets among those `admits` holds for, one of them seen by the
/// viewer, whatever builds it is in.
fn binds_to_differing_targets(
    bindings: SeenBindings,
    name: &str,
    admits: impl Fn(&ReExportTarget) -> bool,
) -> bool {
    private_targets_differ(bindings.every(name), admits)
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
pub(super) fn sees_the_module(bindings: SeenBindings, name: &str) -> bool {
    let is_module = |binding: &NameBinding| binding.kind == BindingKind::Module;
    bindings.named(name).any(is_module) || !bindings.unseen(name).any(is_module)
}

/// The `*` marker on a glob's dependency. It must survive upstream of here:
/// `collect_use_reexports` reads it out of its own `resolve_use_tree` pass to
/// hold the module's globs, which is what `resolve_reexport` resolves glob
/// chains against.
pub(super) fn is_glob(dep: &DependencyRef) -> bool {
    dep.target_item.as_deref() == Some("*")
}

#[cfg(test)]
mod tests;
