//! The dependencies of a file: each path its code writes, resolved to the
//! item it names, with the uses of that item.

use super::super::packages::normalize_crate_name;
use super::super::use_parser::{
    BindingKind, Brought, CrateModule, Meaning, NameBinding, SeenBindings, append_to_path,
    is_reexport_visibility, resolve_use_tree,
};
use super::collect::{
    BindingRegions, CollectedPathRefs, CollectedUse, CollectedUses, LocalBinding, PathPosition,
    PathRef, RegionId, attributes_expand, collect_all_path_refs, collect_all_use_items,
    foreign_item_ident, item_ident,
};
use super::context::ResolutionContext;
use super::lookup::{FromProduction, PassedOn, Visited, covers, is_glob};
use super::path::{PathEnd, PathStart, resolve_single_path};
use crate::model::{
    DefKind, DependencyRef, EdgeContext, SymbolUse, TargetRoot, TestKind, UsageKind, UseCategory,
};
use std::collections::{HashMap, HashSet};
use syn::visit::Visit;

/// Parse syn-based use items, extracting workspace-relevant dependencies.
///
/// Returns `DependencyRefs` for:
/// - Crate-local imports (`use crate::module`)
/// - Workspace crate imports (`use other_crate::module` where `other_crate` is in workspace)
///
/// Deduplicates by `full_target()` to keep distinct symbols but avoid duplicates.
fn parse_workspace_dependencies(
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
struct FileBindings {
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
    fn of(syntax: &syn::File, use_items: &CollectedUses, ctx: &ResolutionContext) -> FileBindings {
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
fn parse_path_ref_dependencies(
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
/// ([`super::context::RootBuilds::reading`]), whatever build `file` reads.
pub(super) fn parse_file_dependencies(
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
mod tests {
    use super::super::super::use_parser::Visibility;
    use super::*;
    use crate::model::Definition;

    #[test]
    fn a_name_defined_public_and_restricted_has_differing_visibilities() {
        let definition = |line| {
            BindingKind::Definition(Definition {
                kind: DefKind::Struct,
                line,
            })
        };
        let public = NameBinding::new(definition(1), Visibility::Public);
        let restricted =
            NameBinding::new(definition(2), Visibility::Restricted("holder".to_string()));

        assert!(definition_visibilities_differ(
            [&restricted, &public].into_iter()
        ));
    }
}
