//! Re-export collection for accurate dependency edges.
//!
//! Collects `pub use` re-exports and own definitions per module so that
//! downstream resolution can follow re-export chains to the original definition.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use syn::spanned::Spanned;

use super::super::packages::{ModulePathMap, normalize_crate_name};
use crate::model::{DefKind, Definition, PackageInfo};

use super::super::mod_resolver::{
    CfgCondition, ModuleFile, declared_files, extract_mod_declarations,
};
use super::super::use_parser::{
    AssociatedItems, BindingKind, Brought, GlobBinding, GlobReach, ModuleExportInfo, NameBinding,
    Namespace, ReExportMap, RootReExports, Visibility, is_reexport_visibility,
};
use super::collect::{attributes_expand, item_attrs};
use super::place::{ExternPrelude, UsePlace, UseScope, resolve_use_tree};

/// Invariant parameters shared across the recursive re-export walk.
struct CollectContext<'a> {
    crate_name: &'a str,
    all_module_paths: &'a ModulePathMap,
    extern_prelude: &'a ExternPrelude,
}

/// Collect the re-exports and own definitions of every package in
/// `packages`, by package key, and settle each `use` of another workspace
/// crate ([`ReExportMap::settle_written_paths`]), so no target in the map
/// still holds a path as written.
pub(super) fn collect_reexport_map(
    packages: &[PackageInfo],
    all_module_paths: &ModulePathMap,
    include_tests: bool,
) -> ReExportMap {
    packages
        .iter()
        .map(|krate| {
            let roots = collect_crate_reexports(krate, all_module_paths, include_tests);
            (krate.key.clone(), roots)
        })
        .collect::<ReExportMap>()
        .settle_written_paths(all_module_paths)
}

/// Collect re-exports and own definitions for all modules of a package,
/// one map per entry file, in `entry_files` order. With `include_tests`, a
/// root whose test build loads other files than its build without `test`
/// also gets the modules of its test build.
fn collect_crate_reexports(
    crate_info: &PackageInfo,
    all_module_paths: &ModulePathMap,
    include_tests: bool,
) -> Vec<RootReExports> {
    let crate_name = normalize_crate_name(&crate_info.name);

    crate_info
        .entry_files
        .iter()
        .map(|entry| {
            let extern_prelude = ExternPrelude::of_root(crate_info, Some(entry));
            let ctx = CollectContext {
                crate_name: &crate_name,
                all_module_paths,
                extern_prelude: &extern_prelude,
            };
            let (modules, test_build_modules) =
                collect_root_reexports(&ctx, &entry.file, include_tests);
            RootReExports {
                kind: entry.kind,
                modules,
                test_build_modules,
            }
        })
        .collect()
}

/// The modules of a root's build without `test`, and with `include_tests` those
/// of its test build where they differ.
fn collect_root_reexports(
    ctx: &CollectContext,
    root_file: &Path,
    include_tests: bool,
) -> (
    HashMap<String, ModuleExportInfo>,
    Option<HashMap<String, ModuleExportInfo>>,
) {
    let mut production = CollectedModules::default();
    let mut test_build = include_tests.then(CollectedModules::default);
    let root = ModuleFile {
        path: root_file.to_path_buf(),
        owns_directory: true,
    };
    walk_collect_reexports(
        ctx,
        &root,
        "",
        Builds {
            without_test: true,
            test: include_tests,
        },
        &mut production,
        test_build.as_mut(),
    );
    let production = production.finish();
    let test_build = test_build
        .map(CollectedModules::finish)
        .filter(|test_build| *test_build != production);
    (production, test_build)
}

/// The modules one build of a root loads, while the walk collects them.
#[derive(Default)]
struct CollectedModules {
    modules: HashMap<String, ModuleExportInfo>,
    out_of_module_impls: Vec<OutOfModuleImpl>,
    /// The `#[macro_export]` macros of every module, which rustc places in
    /// the crate root.
    exported_macros: HashSet<String>,
}

impl CollectedModules {
    /// Hold the exported macros as names of the crate root, drop the modules
    /// that hold nothing a resolution reads, then add each `impl` block to
    /// the module that defines its type.
    fn finish(self) -> HashMap<String, ModuleExportInfo> {
        let mut modules = self.modules;
        if !self.exported_macros.is_empty() {
            modules
                .entry(String::new())
                .or_default()
                .unlisted
                .extend(self.exported_macros);
        }
        modules.retain(|_, info| !info.is_empty());
        for block in self.out_of_module_impls {
            modules
                .entry(block.module)
                .or_default()
                .associated
                .entry(block.type_name)
                .or_default()
                .merge(block.items);
        }
        modules
    }
}

/// Which builds of a root load a file, as far as the `cfg` and `cfg_attr`
/// predicates on the way to it tell. The test build counts only where its
/// modules are collected.
#[derive(Clone, Copy)]
struct Builds {
    without_test: bool,
    test: bool,
}

/// An `impl` block written in another module than the one defining its type.
#[derive(Clone)]
struct OutOfModuleImpl {
    /// The module that defines the type.
    module: String,
    type_name: String,
    items: AssociatedItems,
}

/// Where the type of an `impl` block is defined.
enum ImplOwner {
    ThisModule,
    Other(String),
}

/// Extract re-exports, definitions and associated items from the items of a
/// single module, as the test builds read it where `test` is set and as the
/// builds without `test` read it otherwise: an item under a `#[cfg]` that
/// holds in none of them is left out. An `impl` block whose type lives in
/// another module goes to `out_of_module_impls` for that module.
fn collect_module_info(
    ctx: &CollectContext,
    items: &[syn::Item],
    module_path: &str,
    test: bool,
    out_of_module_impls: &mut Vec<OutOfModuleImpl>,
) -> ModuleExportInfo {
    let mut info = ModuleExportInfo::default();
    let items = items_in_builds(items, test);
    info.expands_macros = items.iter().any(|(item, _)| expands_a_macro(item));
    info.unlisted = unlisted_names(&items);
    let type_items = type_item_names(&items);
    let scope = use_scope(ctx, module_path, test, &type_items);
    let macros = macro_rules_names(&items);

    for &(item, builds) in &items {
        let defined = match item {
            syn::Item::Use(use_item) => {
                collect_use_reexports(
                    ctx,
                    &scope,
                    use_item,
                    module_path,
                    &macros,
                    &mut info,
                    builds,
                );
                None
            }
            syn::Item::Mod(i) => Some((&i.vis, i.ident.clone(), BindingKind::Module)),
            // An attribute macro on the block may remove or rewrite its items.
            syn::Item::ForeignMod(block) if attributes_expand(&block.attrs) => None,
            syn::Item::ForeignMod(block) => {
                bind_foreign_items(&mut info, block, module_path, builds, test);
                None
            }
            _ => defined_item(item).map(|(vis, ident, kind)| {
                let line = item.span().start().line;
                (
                    vis,
                    ident,
                    BindingKind::Definition(Definition { kind, line }),
                )
            }),
        };
        if let Some((vis, ident, kind)) = defined {
            let binding = NameBinding {
                kind,
                visibility: Visibility::of(vis, module_path),
                builds,
            };
            info.bind(ident.to_string(), binding);
        }
    }
    // Second pass, once every `use` of the file is known: an `impl` block may
    // precede the `use` that names its type.
    for &(item, _) in &items {
        match item {
            syn::Item::Enum(e) => {
                let items = info.associated.entry(e.ident.to_string()).or_default();
                items
                    .variants
                    .extend(e.variants.iter().map(|v| v.ident.to_string()));
            }
            syn::Item::Trait(t) => {
                let mut items = AssociatedItems::default();
                for trait_item in &t.items {
                    match trait_item {
                        syn::TraitItem::Fn(f) => items.add_fn(&f.sig.ident),
                        syn::TraitItem::Const(c) => items.add_const(&c.ident),
                        _ => {}
                    }
                }
                info.associated
                    .entry(t.ident.to_string())
                    .or_default()
                    .merge(items);
            }
            syn::Item::Impl(i) => {
                let syn::Type::Path(self_type) = &*i.self_ty else {
                    continue;
                };
                let Some((owner, type_name)) =
                    impl_owner(ctx, &scope, &self_type.path, module_path, &info)
                else {
                    continue;
                };
                let mut items = AssociatedItems::default();
                for impl_item in &i.items {
                    match impl_item {
                        syn::ImplItem::Fn(f) => items.add_fn(&f.sig.ident),
                        syn::ImplItem::Const(c) => items.add_const(&c.ident),
                        _ => {}
                    }
                }
                match owner {
                    ImplOwner::ThisModule => {
                        info.associated.entry(type_name).or_default().merge(items);
                    }
                    ImplOwner::Other(module) => out_of_module_impls.push(OutOfModuleImpl {
                        module,
                        type_name,
                        items,
                    }),
                }
            }
            _ => {}
        }
    }

    info
}

/// Select the `items` the test builds read where `test` is set, and the
/// builds without `test` otherwise, each with whether every one of those
/// builds reads it.
fn items_in_builds(items: &[syn::Item], test: bool) -> Vec<(&syn::Item, Brought)> {
    items
        .iter()
        .filter_map(
            |item| match CfgCondition::of_attrs(item_attrs(item)).in_builds(test) {
                Some(false) => None,
                Some(true) => Some((item, Brought::InEveryBuild)),
                None => Some((item, Brought::InSomeBuilds)),
            },
        )
        .collect()
}

/// The names of the `macro_rules!` among `items`, each with the line its item
/// starts on.
fn macro_rules_names(items: &[(&syn::Item, Brought)]) -> HashMap<String, usize> {
    items
        .iter()
        .filter_map(|(item, _)| match item {
            syn::Item::Macro(i) => i
                .ident
                .as_ref()
                .map(|ident| (ident.to_string(), item.span().start().line)),
            _ => None,
        })
        .collect()
}

/// The names `items` bind by an item no binding lists: a union, and the name
/// an `extern crate` binds.
fn unlisted_names(items: &[(&syn::Item, Brought)]) -> HashSet<String> {
    items
        .iter()
        .filter_map(|(item, _)| match item {
            syn::Item::Union(i) => Some(i.ident.to_string()),
            syn::Item::ExternCrate(i) => Some(
                i.rename
                    .as_ref()
                    .map_or(&i.ident, |(_, name)| name)
                    .to_string(),
            ),
            _ => None,
        })
        .collect()
}

/// Bind each item the `extern` block `block` in `module_path` declares as a
/// definition of the module, in the builds of the map that read it, where
/// `block_builds` of them read the block.
fn bind_foreign_items(
    info: &mut ModuleExportInfo,
    block: &syn::ItemForeignMod,
    module_path: &str,
    block_builds: Brought,
    test: bool,
) {
    for (foreign, builds) in foreign_items_in_builds(block, block_builds, test) {
        let Some((vis, ident, kind)) = declared_foreign_item(foreign) else {
            continue;
        };
        let line = foreign.span().start().line;
        let binding = NameBinding {
            kind: BindingKind::Definition(Definition { kind, line }),
            visibility: Visibility::of(vis, module_path),
            builds,
        };
        info.bind(ident.to_string(), binding);
    }
}

/// Select the items of the `extern` block `block`, which `block_builds` of
/// the builds `test` selects read, that a build reads, each with whether
/// every one of those builds reads it.
fn foreign_items_in_builds(
    block: &syn::ItemForeignMod,
    block_builds: Brought,
    test: bool,
) -> impl Iterator<Item = (&syn::ForeignItem, Brought)> {
    block.items.iter().filter_map(move |item| {
        let attrs = match item {
            syn::ForeignItem::Fn(i) => &i.attrs,
            syn::ForeignItem::Static(i) => &i.attrs,
            syn::ForeignItem::Type(i) => &i.attrs,
            syn::ForeignItem::Macro(i) => &i.attrs,
            _ => return Some((item, block_builds)),
        };
        match CfgCondition::of_attrs(attrs).in_builds(test) {
            Some(false) => None,
            Some(true) => Some((item, block_builds)),
            None => Some((item, Brought::InSomeBuilds)),
        }
    })
}

/// The visibility, name and kind of an item an `extern` block declares, which
/// rustc places in the module that holds the block; `None` for a macro call
/// and for an item under an attribute macro, which may remove or rewrite it.
fn declared_foreign_item(
    item: &syn::ForeignItem,
) -> Option<(&syn::Visibility, &syn::Ident, DefKind)> {
    let (attrs, declared) = match item {
        syn::ForeignItem::Fn(i) => (&i.attrs, (&i.vis, &i.sig.ident, DefKind::Fn)),
        syn::ForeignItem::Static(i) => (&i.attrs, (&i.vis, &i.ident, DefKind::Static)),
        syn::ForeignItem::Type(i) => (&i.attrs, (&i.vis, &i.ident, DefKind::Type)),
        _ => return None,
    };
    (!attributes_expand(attrs)).then_some(declared)
}

/// Whether `item` calls a macro, carries an attribute or a derive macro that
/// rustc does not build in, or is code syn does not parse: the expansion may
/// define names in the module.
fn expands_a_macro(item: &syn::Item) -> bool {
    let foreign_expands = |block: &syn::ItemForeignMod| {
        block.items.iter().any(|item| match item {
            syn::ForeignItem::Fn(i) => attributes_expand(&i.attrs),
            syn::ForeignItem::Static(i) => attributes_expand(&i.attrs),
            syn::ForeignItem::Type(i) => attributes_expand(&i.attrs),
            _ => true,
        })
    };
    match item {
        syn::Item::Macro(i) => i.ident.is_none() || attributes_expand(&i.attrs),
        syn::Item::Verbatim(_) => true,
        syn::Item::ForeignMod(block) => attributes_expand(&block.attrs) || foreign_expands(block),
        _ => attributes_expand(item_attrs(item)),
    }
}

/// The names the type items among `items` define.
fn type_item_names(items: &[(&syn::Item, Brought)]) -> HashSet<String> {
    items
        .iter()
        .filter_map(|(item, _)| defined_item(item))
        .filter(|(_, _, kind)| Namespace::of(*kind) == Namespace::Type)
        .map(|(_, ident, _)| ident.to_string())
        .collect()
}

/// The visibility, name and kind of an item that defines a symbol, or `None`
/// for any other item.
fn defined_item(item: &syn::Item) -> Option<(&syn::Visibility, syn::Ident, DefKind)> {
    let (vis, ident, kind) = match item {
        syn::Item::Fn(i) => {
            let (name, kind) = name_a_fn_defines(i);
            return Some((&i.vis, name, kind));
        }
        syn::Item::Struct(i) => (&i.vis, &i.ident, DefKind::Struct),
        syn::Item::Enum(i) => (&i.vis, &i.ident, DefKind::Enum),
        syn::Item::Trait(i) => (&i.vis, &i.ident, DefKind::Trait),
        syn::Item::Const(i) => (&i.vis, &i.ident, DefKind::Const),
        syn::Item::Static(i) => (&i.vis, &i.ident, DefKind::Static),
        syn::Item::Type(i) => (&i.vis, &i.ident, DefKind::Type),
        _ => return None,
    };
    Some((vis, ident.clone(), kind))
}

/// The name a function defines and what it is. A `#[proc_macro_derive(Name)]`
/// defines the derive macro `Name`, not a function under its own name; a
/// `#[proc_macro]` or `#[proc_macro_attribute]` defines a macro under its own
/// name.
pub(super) fn name_a_fn_defines(item: &syn::ItemFn) -> (syn::Ident, DefKind) {
    for attr in &item.attrs {
        let path = attr.path();
        if path.is_ident("proc_macro_derive") {
            let derive_name = attr.parse_args_with(|input: syn::parse::ParseStream| {
                let name: syn::Ident = input.parse()?;
                input.parse::<proc_macro2::TokenStream>()?;
                Ok(name)
            });
            if let Ok(name) = derive_name {
                return (name, DefKind::Macro);
            }
        } else if path.is_ident("proc_macro") || path.is_ident("proc_macro_attribute") {
            return (item.sig.ident.clone(), DefKind::Macro);
        }
    }
    (item.sig.ident.clone(), DefKind::Fn)
}

/// Find the module that defines the type an `impl` block is for, and the
/// type's name there. A bare name is this module's when it defines the name,
/// and otherwise whatever a `use` of this file binds it to; a qualified path
/// is placed like the path of a `use`, in the test build where `test` is
/// set. What resolves to neither, or to a path the module tree alone cannot
/// place, is no type of this crate, and its `impl` is skipped.
fn impl_owner(
    ctx: &CollectContext,
    scope: &UseScope,
    path: &syn::Path,
    module_path: &str,
    info: &ModuleExportInfo,
) -> Option<(ImplOwner, String)> {
    let name = path.segments.last()?.ident.to_string();
    if path.segments.len() == 1 {
        let named = info.named(&name);
        if named
            .clone()
            .any(|binding| matches!(binding.kind, BindingKind::Definition(_)))
        {
            return Some((ImplOwner::ThisModule, name));
        }
        let bound = named.clone().find_map(NameBinding::use_target)?;
        return Some((
            ImplOwner::Other(bound.module.clone()),
            bound.original_name.clone(),
        ));
    }
    let path_str: String = path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    let Some(UsePlace::Name {
        target,
        from_scope: false,
    }) = scope.place(&path_str)
    else {
        return None;
    };
    if target.crate_name != ctx.crate_name
        || !ctx.all_module_paths.holds(ctx.crate_name, &target.module)
    {
        return None;
    }
    let owner = if target.module == module_path {
        ImplOwner::ThisModule
    } else {
        ImplOwner::Other(target.module)
    };
    Some((owner, target.original_name))
}

/// The scope that places the paths written in `module_path`, which defines
/// `type_items`, in the test build where `test` is set.
fn use_scope<'a>(
    ctx: &'a CollectContext,
    module_path: &'a str,
    test: bool,
    type_items: &'a HashSet<String>,
) -> UseScope<'a> {
    UseScope {
        crate_name: ctx.crate_name,
        module_path,
        module_paths: ctx.all_module_paths,
        extern_prelude: ctx.extern_prelude.names(test),
        type_items,
    }
}

/// Place a single `use` item into re-export entries, each with the
/// visibility of the `use`. Each path is placed by the module tree alone
/// ([`UseScope::place`]); no binding of the table is read while the table is
/// collected.
///
/// Its visibility selects the kind of binding a named target gets: a
/// `pub`/`pub(crate)`/`pub(super)` re-export binds by
/// [`BindingKind::ReExport`], a private, `pub(self)` or `pub(in …)` `use` by
/// [`BindingKind::PrivateUse`] (a binding descendants can still name through
/// this module). A glob is held with [`GlobReach::ReExport`] or
/// [`GlobReach::Private`] by the same split, with the crate of its module
/// where that is another workspace crate, and a glob no start places as one
/// of the module's unplaced globs. A non-glob leaf that no start places
/// names something outside the workspace. Private, it binds by
/// [`BindingKind::Elsewhere`], which keeps a descendant's reference to it
/// from resolving to this module; public, by
/// [`BindingKind::ReExportFromOutside`]. A `use name;` of one of `macros`, the file's own
/// `macro_rules!`, binds that macro here as a definition. Renamed, it may also
/// bind what another `use` brought in under `name`, so it stays in. Each
/// binding is in the `builds` of the map the `use` is in, the test build's
/// map where `test` is set.
fn collect_use_reexports(
    ctx: &CollectContext,
    scope: &UseScope,
    use_item: &syn::ItemUse,
    module_path: &str,
    macros: &HashMap<String, usize>,
    info: &mut ModuleExportInfo,
    builds: Brought,
) {
    let alias_paths = resolve_use_tree(&use_item.tree, "", true);
    let original_paths = resolve_use_tree(&use_item.tree, "", false);

    let visibility = Visibility::of(&use_item.vis, module_path);
    let private = !is_reexport_visibility(&use_item.vis);

    for (alias_path, original_path) in alias_paths.iter().zip(original_paths.iter()) {
        let alias_name = alias_path.rsplit("::").next().unwrap_or(alias_path);
        let (kind, from_scope) = match scope.place(original_path) {
            None => {
                if alias_name == "*" {
                    info.unplaced_globs.push(visibility.clone());
                    continue;
                }
                let own_macro = macros
                    .get(original_path)
                    .filter(|_| alias_path == original_path);
                let kind = if let Some(&line) = own_macro {
                    BindingKind::Definition(Definition {
                        kind: DefKind::Macro,
                        line,
                    })
                } else if private {
                    BindingKind::Elsewhere
                } else {
                    BindingKind::ReExportFromOutside(original_path.clone())
                };
                (kind, false)
            }
            Some(UsePlace::Glob { crate_name, module }) => {
                let foreign = crate_name != ctx.crate_name;
                info.hold_glob(GlobBinding {
                    reach: if private {
                        GlobReach::Private
                    } else {
                        GlobReach::ReExport
                    },
                    crate_name,
                    module,
                    foreign,
                    visibility: visibility.clone(),
                });
                continue;
            }
            Some(UsePlace::Name { target, from_scope }) => {
                let kind = if private {
                    BindingKind::PrivateUse(target)
                } else {
                    BindingKind::ReExport(target)
                };
                (kind, from_scope)
            }
            Some(UsePlace::Nothing) => continue,
        };
        let binding = NameBinding {
            kind,
            visibility: visibility.clone(),
            builds,
        };
        if from_scope {
            info.bind_from_scope(alias_name.to_string(), binding);
        } else {
            info.bind(alias_name.to_string(), binding);
        }
    }
}

/// Collect the module at `module_path`, whose items are `items`, into
/// `modules`, and each module these declare inline with a body, under its
/// path below `module_path`.
fn collect_module(
    ctx: &CollectContext,
    items: &[syn::Item],
    module_path: &str,
    test: bool,
    modules: &mut CollectedModules,
) {
    let info = collect_module_info(
        ctx,
        items,
        module_path,
        test,
        &mut modules.out_of_module_impls,
    );
    let exported = items_in_builds(items, test)
        .into_iter()
        .filter_map(|(item, _)| match item {
            syn::Item::Macro(i)
                if i.attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("macro_export")) =>
            {
                i.ident.as_ref().map(ToString::to_string)
            }
            _ => None,
        });
    modules.exported_macros.extend(exported);
    // Merged even when empty: a public declaration widens what an
    // alternative file of the module declares narrower.
    modules
        .modules
        .entry(module_path.to_string())
        .or_default()
        .merge(info);
    for item in items {
        if let syn::Item::Mod(item_mod) = item
            && let Some((_, content)) = &item_mod.content
            && CfgCondition::of_attrs(&item_mod.attrs).in_builds(test) != Some(false)
        {
            let inline_path = if module_path.is_empty() {
                item_mod.ident.to_string()
            } else {
                format!("{module_path}::{}", item_mod.ident)
            };
            collect_module(ctx, content, &inline_path, test, modules);
        }
    }
}

/// Read `file` and its descendants once, each into the modules of the builds
/// that load it: `production` for the build without `test`, `test_build`,
/// where given, for the test build. Each build takes the items of the file
/// that its `#[cfg]`s admit.
fn walk_collect_reexports(
    ctx: &CollectContext,
    file: &ModuleFile,
    module_path: &str,
    builds: Builds,
    production: &mut CollectedModules,
    mut test_build: Option<&mut CollectedModules>,
) {
    let file_path = file.path.as_path();
    let source = match std::fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("skipping {}: {e:#}", file_path.display());
            return;
        }
    };
    let syntax = match syn::parse_file(&source) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("parsing {}: {e:#}", file_path.display());
            return;
        }
    };

    let collect_into = |modules: &mut CollectedModules, test: bool| {
        collect_module(ctx, &syntax.items, module_path, test, modules);
    };
    if let Some(test_build) = test_build.as_deref_mut().filter(|_| builds.test) {
        collect_into(test_build, true);
    }
    if builds.without_test {
        collect_into(production, false);
    }

    // Recurse into child modules
    let decls = extract_mod_declarations(&syntax, test_build.is_some());

    for decl in decls {
        let child_module_path = if module_path.is_empty() {
            decl.name.clone()
        } else {
            format!("{module_path}::{}", decl.name)
        };

        for child in declared_files(file_path, file.owns_directory, &decl) {
            let child_builds = Builds {
                without_test: builds.without_test
                    && !decl.condition.test_builds_only()
                    && !child.condition.test_builds_only(),
                test: builds.test
                    && decl.condition.may_hold_in_test_build()
                    && child.condition.may_hold_in_test_build(),
            };
            if !child_builds.without_test && !child_builds.test {
                continue;
            }
            walk_collect_reexports(
                ctx,
                &child.file,
                &child_module_path,
                child_builds,
                production,
                test_build.as_deref_mut(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::test_workspace::{Analyzed, TestWorkspace};
    use super::*;
    use crate::model::{DependencyRef, SymbolUse, UseCategory};
    use crate::test_support::conventional_package;
    use std::collections::HashSet;
    use tempfile::TempDir;

    /// Analyze `test_crate` whose root is `root`, beside the further `files`.
    fn test_crate(root: &str, files: &[(&str, &str)]) -> Analyzed {
        let mut all = vec![("src/lib.rs", root)];
        all.extend_from_slice(files);
        TestWorkspace::new().package("test_crate", &all).analyze()
    }

    /// The names the module at `path` defines, sorted.
    fn defined_names(ws: &Analyzed, path: &str) -> Vec<String> {
        let mut names: Vec<String> = ws.module(path).definitions.keys().cloned().collect();
        names.sort();
        names
    }

    /// The items `user`, a crate of its own that globs every name of
    /// `test_crate`'s root, takes from `test_crate`, sorted, without the
    /// glob's own arc.
    fn globbed_from(test_crate_root: &str, files: &[(&str, &str)], user: &str) -> Vec<String> {
        let mut all = vec![("src/lib.rs", test_crate_root)];
        all.extend_from_slice(files);
        let ws = TestWorkspace::new()
            .package("test_crate", &all)
            .package("user", &[("src/lib.rs", user)])
            .analyze();
        let mut items: Vec<String> = ws
            .deps("user")
            .iter()
            .filter(|dep| dep.target_package == "test_crate")
            .filter(|dep| dep.target_item.as_deref() != Some("*"))
            .map(DependencyRef::full_target)
            .collect();
        items.sort();
        items
    }

    // Cycle 5: pub use sibling::Item
    #[test]
    fn collects_pub_use_sibling_item() {
        let ws = test_crate(
            "pub mod parent;\nmod user;",
            &[
                (
                    "src/parent/mod.rs",
                    "pub mod sibling;\npub use sibling::Item;",
                ),
                ("src/parent/sibling.rs", "pub struct Item;"),
                ("src/user.rs", "use crate::parent::Item;"),
            ],
        );
        assert_eq!(
            ws.targets("test_crate::user"),
            [("test_crate", "parent::sibling", Some("Item"))]
        );
    }

    // Cycle 6: pub use crate::model::Config
    #[test]
    fn collects_pub_use_crate_module_item() {
        let ws = test_crate(
            "pub mod model;\nmod user;\npub use crate::model::Config;",
            &[
                ("src/model.rs", "pub struct Config;"),
                ("src/user.rs", "use crate::Config;"),
            ],
        );
        assert_eq!(
            ws.targets("test_crate::user"),
            [("test_crate", "model", Some("Config"))]
        );
    }

    // Cycle 7: pub use super::sibling::Item
    #[test]
    fn collects_pub_use_super_sibling_item() {
        let ws = test_crate(
            "pub mod parent;\nmod user;",
            &[
                ("src/parent/mod.rs", "pub mod sibling;\npub mod child;"),
                ("src/parent/sibling.rs", "pub struct Item;"),
                ("src/parent/child.rs", "pub use super::sibling::Item;"),
                ("src/user.rs", "use crate::parent::child::Item;"),
            ],
        );
        assert_eq!(
            ws.targets("test_crate::user"),
            [("test_crate", "parent::sibling", Some("Item"))]
        );
    }

    // Cycle 8: pub use elements::*
    #[test]
    fn collects_pub_use_glob() {
        let ws = test_crate(
            "pub mod render;\nmod user;",
            &[
                (
                    "src/render/mod.rs",
                    "pub mod elements;\npub use elements::*;",
                ),
                ("src/render/elements.rs", "pub struct Widget;"),
                ("src/user.rs", "use crate::render::Widget;"),
            ],
        );
        assert_eq!(
            ws.targets("test_crate::user"),
            [("test_crate", "render::elements", Some("Widget"))]
        );
    }

    // Cycle 9: pub use sibling::{A, B}
    #[test]
    fn collects_pub_use_group_import() {
        let ws = test_crate(
            "pub mod parent;\nmod user;",
            &[
                (
                    "src/parent/mod.rs",
                    "pub mod sibling;\npub use sibling::{Alpha, Beta};",
                ),
                (
                    "src/parent/sibling.rs",
                    "pub struct Alpha;\npub struct Beta;",
                ),
                ("src/user.rs", "use crate::parent::{Alpha, Beta};"),
            ],
        );
        assert_eq!(
            ws.sorted_targets("test_crate::user"),
            [
                ("test_crate", "parent::sibling", Some("Alpha")),
                ("test_crate", "parent::sibling", Some("Beta")),
            ]
        );
    }

    /// `parent` binds `Private` with a private `use` from its child `sibling`,
    /// `parent::kid` is `kid` and the module `user` outside `parent` is `user`.
    fn with_private_binding_in_parent(parent_use: &str, kid: &str, user: &str) -> Analyzed {
        test_crate(
            "pub mod parent;\nmod user;",
            &[
                (
                    "src/parent/mod.rs",
                    &format!("pub mod sibling;\nmod kid;\n{parent_use}"),
                ),
                (
                    "src/parent/sibling.rs",
                    "pub struct Public;\npub struct Private;",
                ),
                ("src/parent/kid.rs", kid),
                ("src/user.rs", user),
            ],
        )
    }

    // Cycle 10: pub(crate) use captured, private use NOT
    #[test]
    fn pub_crate_use_captured_private_not() {
        let ws = with_private_binding_in_parent(
            "pub(crate) use sibling::Public;\nuse sibling::Private;",
            "",
            "use crate::parent::{Public, Private};",
        );
        assert_eq!(
            ws.sorted_targets("test_crate::user"),
            [
                ("test_crate", "parent", Some("Private")),
                ("test_crate", "parent::sibling", Some("Public")),
            ],
            "a pub(crate) use passes the name on, a private use not beyond its module"
        );
    }

    // A private use binds as such, where a descendant can still resolve it
    #[test]
    fn private_use_captured_as_a_private_use() {
        let ws = with_private_binding_in_parent(
            "use sibling::Private;",
            "use super::Private;",
            "use crate::parent::Private;",
        );
        assert_eq!(
            ws.targets("test_crate::parent::kid"),
            [("test_crate", "parent::sibling", Some("Private"))]
        );
        assert_eq!(
            ws.targets("test_crate::user"),
            [("test_crate", "parent", Some("Private"))],
            "private use must not leak into the re-exports"
        );
    }

    // A private glob is held as private, where a descendant can still resolve
    // the names it forwards
    #[test]
    fn private_glob_held_as_private() {
        let ws = with_private_binding_in_parent(
            "use sibling::*;",
            "use super::Private;",
            "use crate::parent::Private;",
        );
        assert_eq!(
            ws.targets("test_crate::parent::kid"),
            [("test_crate", "parent::sibling", Some("Private"))]
        );
        assert_eq!(
            ws.targets("test_crate::user"),
            [("test_crate", "parent", Some("Private"))],
            "the private glob is no re-export"
        );
    }

    // Cycle 11: pub struct Foo → definitions
    #[test]
    fn collects_pub_struct_as_definition() {
        let ws = test_crate(
            "pub mod module;",
            &[("src/module.rs", "pub struct Foo;\npub fn helper() {}")],
        );
        assert_eq!(defined_names(&ws, "test_crate::module"), ["Foo", "helper"]);
    }

    /// Characterization: which item kinds bind as definitions, whatever their
    /// visibility.
    #[test]
    fn definitions_cover_all_matched_item_kinds() {
        let ws = test_crate(
            "pub mod module;",
            &[(
                "src/module.rs",
                r"
pub fn a_fn() {}
pub struct AStruct;
pub enum AnEnum { V }
pub trait ATrait {}
pub const A_CONST: u8 = 1;
pub static A_STATIC: u8 = 1;
pub type AnAlias = u8;
pub union AUnion { f: u8 }
fn private_fn() {}
struct PrivateStruct;
pub(crate) struct CrateStruct;
pub(super) struct SuperStruct;
pub(in crate::module) struct InPathStruct;
impl AStruct { pub fn method(&self) {} }
",
            )],
        );
        let definitions = &ws.module("test_crate::module").definitions;
        let mut kinds: Vec<(&str, DefKind)> = definitions
            .iter()
            .map(|(name, definition)| (name.as_str(), definition.kind))
            .collect();
        kinds.sort_by_key(|(name, _)| *name);
        assert_eq!(
            kinds,
            [
                ("AStruct", DefKind::Struct),
                ("ATrait", DefKind::Trait),
                ("A_CONST", DefKind::Const),
                ("A_STATIC", DefKind::Static),
                ("AnAlias", DefKind::Type),
                ("AnEnum", DefKind::Enum),
                ("CrateStruct", DefKind::Struct),
                ("InPathStruct", DefKind::Struct),
                ("PrivateStruct", DefKind::Struct),
                ("SuperStruct", DefKind::Struct),
                ("a_fn", DefKind::Fn),
                ("private_fn", DefKind::Fn),
            ],
            "no AUnion, method or V"
        );
    }

    /// A function of a proc-macro crate defines a macro: a derive under the
    /// name its attribute gives, the others under the function's own name.
    #[test]
    fn proc_macro_functions_define_macros() {
        let source = r"
#[proc_macro_derive(Widget, attributes(widget))]
pub fn derive_widget(input: TokenStream) -> TokenStream { input }
#[proc_macro]
pub fn make(input: TokenStream) -> TokenStream { input }
#[proc_macro_attribute]
pub fn wrap(_: TokenStream, input: TokenStream) -> TokenStream { input }
";
        let ws = TestWorkspace::new()
            .package("test_crate", &[("src/lib.rs", source)])
            .package(
                "user",
                &[("src/lib.rs", "use test_crate::{Widget, make, wrap};")],
            )
            .analyze();
        let definitions = &ws.module("test_crate").definitions;
        for name in ["Widget", "make", "wrap"] {
            assert_eq!(
                definitions.get(name).map(|definition| definition.kind),
                Some(DefKind::Macro),
                "wrong kind for {name}"
            );
        }
        assert!(
            !definitions.contains_key("derive_widget"),
            "{definitions:?}"
        );
        assert_eq!(
            ws.sorted_targets("user"),
            [
                ("test_crate", "", Some("Widget")),
                ("test_crate", "", Some("make")),
                ("test_crate", "", Some("wrap")),
            ],
            "the macros are the crate's entry points"
        );
    }

    /// A private item in the root is visible to every module of the crate,
    /// but not to other crates.
    #[test]
    fn private_root_items_are_recorded_as_restricted() {
        let root = "pub mod module;\nstruct Hidden;\nfn helper() {}\npub struct Shown;\n";
        let files = [(
            "src/module.rs",
            "fn f(_: crate::Hidden) { crate::helper(); }",
        )];
        let from_outside = globbed_from(
            root,
            &files,
            "use test_crate::*;\nfn f(_: Hidden, _: Shown) { helper(); }",
        );
        assert_eq!(from_outside, ["test_crate::Shown"]);
        let ws = test_crate(root, &files);
        assert_eq!(
            ws.sorted_targets("test_crate::module"),
            [
                ("test_crate", "", Some("Hidden")),
                ("test_crate", "", Some("helper")),
            ]
        );
    }

    /// `pub(in …) use name;` passes on the module's own `macro_rules! name`:
    /// a sibling naming the macro through the module depends on that module.
    #[test]
    fn restricted_use_of_the_modules_own_macro_keeps_the_edge() {
        let ws = test_crate(
            "pub mod tools;\n",
            &[
                ("src/tools/mod.rs", "mod expand;\nmod user;\n"),
                (
                    "src/tools/expand.rs",
                    "macro_rules! expand { () => {} }\npub(in crate::tools) use expand;\n",
                ),
                ("src/tools/user.rs", "use super::expand::expand;"),
            ],
        );
        assert_eq!(
            ws.targets("test_crate::tools::user"),
            [("test_crate", "tools::expand", Some("expand"))]
        );
    }

    #[test]
    fn private_child_modules_are_recorded() {
        let found = globbed_from(
            "pub mod shown;\npub(crate) mod inner;\nmod hidden;\npub(self) mod scoped;\n",
            &[
                ("src/shown.rs", "pub struct X;"),
                ("src/inner.rs", "pub struct X;"),
                ("src/hidden.rs", "pub struct X;"),
                ("src/scoped.rs", "pub struct X;"),
            ],
            "use test_crate::*;\nfn f(_: shown::X, _: inner::X, _: hidden::X, _: scoped::X) {}",
        );
        assert_eq!(found, ["test_crate::shown::X"]);
    }

    /// `extern crate other_crate as oc;` in the root lets the root re-export
    /// from `oc`, which names `other_crate`.
    #[test]
    fn root_reexport_through_extern_crate_alias() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod thing;"),
                    ("src/thing.rs", "pub struct Thing;"),
                ],
            )
            .package(
                "test_crate",
                &[
                    (
                        "src/lib.rs",
                        "extern crate other_crate as oc;\nmod user;\npub use oc::thing::Thing;\n",
                    ),
                    ("src/user.rs", "use crate::Thing;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("test_crate::user"),
            [("other_crate", "thing", Some("Thing"))]
        );
    }

    /// `pub(self)` and `pub(in …)` in the root reach no further than the
    /// crate, like a private item.
    #[test]
    fn restricted_root_items_are_recorded_as_private() {
        let found = globbed_from(
            "pub mod module;\npub(self) struct SelfScoped;\npub(in crate) fn in_crate() {}\n",
            &[("src/module.rs", "")],
            "use test_crate::*;\nfn f(_: SelfScoped) { in_crate(); }",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    /// Each definition records the line its item starts on, attributes
    /// included, so a jump lands on the item as the reader sees it.
    #[test]
    fn definitions_carry_the_line_of_the_defining_item() {
        let ws = test_crate(
            "pub mod module;",
            &[(
                "src/module.rs",
                "//! Module doc\n\npub fn a_fn() {}\n\n#[derive(Debug)]\npub struct AStruct;\n",
            )],
        );
        let definitions = &ws.module("test_crate::module").definitions;
        let line = |name| definitions.get(name).map(|definition| definition.line);
        assert_eq!(line("a_fn"), Some(3));
        assert_eq!(line("AStruct"), Some(5));
    }

    /// Characterization: definitions are top-level only — items inside an
    /// inline `mod { … }` body are not collected for any module path.
    #[test]
    fn definitions_skip_inline_mod_bodies() {
        let ws = test_crate(
            "pub mod module;",
            &[(
                "src/module.rs",
                "pub struct Outer;\npub mod inner { pub struct Inner; }",
            )],
        );
        assert_eq!(defined_names(&ws, "test_crate::module"), ["Outer"]);
    }

    // Cycle 12: pub use sibling::Item as Widget (rename)
    #[test]
    fn collects_pub_use_rename() {
        let ws = test_crate(
            "pub mod parent;\nmod user;",
            &[
                (
                    "src/parent/mod.rs",
                    "pub mod sibling;\npub use sibling::Item as Widget;",
                ),
                ("src/parent/sibling.rs", "pub struct Item;"),
                ("src/user.rs", "use crate::parent::{Item, Widget};"),
            ],
        );
        assert_eq!(
            ws.sorted_targets("test_crate::user"),
            [
                ("test_crate", "parent", Some("Item")),
                ("test_crate", "parent::sibling", Some("Item")),
            ],
            "the re-export goes by its new name, the original name is not bound"
        );
    }

    fn use_at(line: usize, category: UseCategory) -> SymbolUse {
        SymbolUse {
            line,
            category,
            imported_for_methods: false,
        }
    }

    /// The uses of `item` in the dependencies of the module at `path`.
    fn uses_of<'a>(ws: &'a Analyzed, path: &str, item: &str) -> &'a [SymbolUse] {
        &ws.dep_on(path, item).uses
    }

    /// Variants, `impl` fns and consts, and trait fns are recorded on the module
    /// that defines the type. An `impl` block in a sibling file lands there too.
    /// A read of an associated item the module records takes the category of
    /// its kind; one it does not record goes by its spelling. The names here
    /// go against the spelling, so that only the record decides.
    #[test]
    fn collects_associated_items_on_the_defining_module() {
        let ws = test_crate(
            "pub mod back;\nmod user;",
            &[
                ("src/back/mod.rs", "pub mod writer;\npub mod features;"),
                (
                    "src/back/writer.rs",
                    "pub enum Mode { FAST, SLOW(u8) }\n\
                     pub struct Writer;\n\
                     impl Writer { pub fn New() -> Self { Writer } pub const max: u8 = 1; }\n\
                     pub trait Step { fn Run(&self); }\n\
                     impl Step for Writer { fn Run(&self) {} }",
                ),
                (
                    "src/back/features.rs",
                    "use super::writer::Writer;\nimpl Writer { pub fn WriteFeature(&self) {} }",
                ),
                (
                    "src/user.rs",
                    "use crate::back::writer::{Mode, Step, Writer};\n\
                     fn f() {\n\
                     let a = Writer::New;\n\
                     let b = Writer::Run;\n\
                     let c = Writer::WriteFeature;\n\
                     let d = Writer::max;\n\
                     let e = Writer::Unknown;\n\
                     let g = Step::Run;\n\
                     let h = Mode::FAST;\n\
                     let i = Mode::SLOW;\n\
                     }",
                ),
            ],
        );
        let user = "test_crate::user";
        assert_eq!(
            uses_of(&ws, user, "Writer"),
            [
                use_at(3, UseCategory::Fns),
                use_at(4, UseCategory::Fns),
                use_at(5, UseCategory::Fns),
                use_at(6, UseCategory::Values),
                use_at(7, UseCategory::Types),
            ],
            "own impl, trait impl and the sibling file's impl; `Unknown` by spelling"
        );
        assert_eq!(uses_of(&ws, user, "Step"), [use_at(8, UseCategory::Fns)]);
        assert_eq!(
            uses_of(&ws, user, "Mode"),
            [
                use_at(9, UseCategory::Types),
                use_at(10, UseCategory::Types)
            ]
        );
        assert!(
            ws.deps(user)
                .iter()
                .all(|dep| dep.target_module == "back::writer"),
            "the impl in features is Writer's, not a type of features: {:?}",
            ws.deps(user)
        );
    }

    /// A private `use super::Name` of a root item passes the name on to the
    /// descendants of its module.
    #[test]
    fn a_use_of_a_root_item_through_super_passes_it_on() {
        let ws = test_crate(
            "pub struct Gauge;\nmod meter;",
            &[
                ("src/meter.rs", "use super::Gauge;\nmod dial;"),
                ("src/meter/dial.rs", "use super::Gauge;"),
            ],
        );
        assert_eq!(
            ws.targets("test_crate::meter::dial"),
            [("test_crate", "", Some("Gauge"))]
        );
    }

    /// A glob of the variants of an enum brings in no other name of the
    /// enum's module.
    #[test]
    fn a_glob_of_variants_brings_no_name_of_their_module() {
        let ws = test_crate(
            "mod kinds;\nmod holder;",
            &[
                ("src/kinds.rs", "pub enum Mode { Fast }\npub struct Other;"),
                ("src/holder.rs", "use crate::kinds::Mode::*;\nmod child;"),
                ("src/holder/child.rs", "use super::Other;"),
            ],
        );
        let child_targets = ws.targets("test_crate::holder::child");
        assert!(
            !child_targets.contains(&("test_crate", "kinds", Some("Other"))),
            "{child_targets:?}"
        );
    }

    /// An `impl` block that names its type as `crate::Name` from another
    /// module adds to the items of the root's `Name`.
    #[test]
    fn an_impl_of_a_root_type_through_crate_lands_on_the_root() {
        let ws = test_crate(
            "pub struct Gauge;\nmod ext;\nmod user;",
            &[
                ("src/ext.rs", "impl crate::Gauge { pub fn Read() {} }"),
                ("src/user.rs", "fn f() { let a = crate::Gauge::Read; }"),
            ],
        );
        assert_eq!(
            uses_of(&ws, "test_crate::user", "Gauge"),
            [use_at(1, UseCategory::Fns)]
        );
    }

    /// An `impl` of a name that a `pub use` and a private `use` bind under
    /// alternative `cfg`s lands where the `pub use` points, the order in
    /// which every lookup reads the bindings of a name.
    #[test]
    fn associated_items_of_a_name_bound_twice_land_where_the_pub_use_points() {
        let ws = test_crate(
            "pub mod a;\npub mod b;\npub mod user;\nmod reader;",
            &[
                ("src/a.rs", "pub struct S;"),
                ("src/b.rs", "pub struct S;"),
                (
                    "src/user.rs",
                    "#[cfg(feature = \"x\")]\npub use crate::a::S;\n\
                     #[cfg(not(feature = \"x\"))]\nuse crate::b::S;\n\
                     impl S { pub fn Made() {} }",
                ),
                (
                    "src/reader.rs",
                    "fn f() {\nlet a = crate::a::S::Made;\nlet b = crate::b::S::Made;\n}",
                ),
            ],
        );
        let uses_in = |module: &str| {
            ws.deps("test_crate::reader")
                .iter()
                .find(|dep| dep.target_module == module)
                .map(|dep| dep.uses.clone())
        };
        assert_eq!(
            (uses_in("a"), uses_in("b")),
            (
                Some(vec![use_at(2, UseCategory::Fns)]),
                Some(vec![use_at(3, UseCategory::Types)])
            ),
            "`Made` is recorded on `a::S` only, `b::S::Made` goes by its spelling"
        );
    }

    // A name the root only re-exports is not known while the map is built, so
    // a `use crate::Name` records the root as its target and the resolution
    // follows the root's re-export later.
    #[test]
    fn use_of_a_name_the_root_reexports_targets_the_root() {
        let ws = test_crate(
            "pub mod span;\npub mod value;\npub mod front;\n\
             pub use crate::span::Span;\npub use value::*;",
            &[
                ("src/span.rs", "pub struct Span;"),
                ("src/value.rs", "pub struct Value;"),
                ("src/front/mod.rs", "pub mod glsl;\npub mod prelude;"),
                ("src/front/glsl/mod.rs", "use crate::Span;"),
                ("src/front/prelude.rs", "pub use crate::Value;"),
            ],
        );
        assert_eq!(
            ws.targets("test_crate::front::glsl"),
            [("test_crate", "span", Some("Span"))]
        );
        assert_eq!(
            ws.targets("test_crate::front::prelude"),
            [("test_crate", "value", Some("Value"))]
        );
    }

    /// `other_crate`'s root brings `Type` in only through `pub use ty::*`;
    /// `test_crate` is `test_crate_root` beside the further `files`, and its
    /// module `user` is `user`.
    fn with_other_root_glob(test_crate_root: &str, files: &[(&str, &str)]) -> Analyzed {
        let mut all = vec![("src/lib.rs", test_crate_root)];
        all.extend_from_slice(files);
        TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod ty;\npub use ty::*;"),
                    ("src/ty.rs", "pub struct Type;"),
                ],
            )
            .package("test_crate", &all)
            .analyze()
    }

    // A name another crate's root only brings in through a glob is unknown
    // while the map is built; the re-export records that root as its target.
    #[test]
    fn reexport_of_a_name_another_root_reexports_targets_that_root() {
        let ws = with_other_root_glob(
            "pub mod prelude;\nmod user;\n",
            &[
                ("src/prelude.rs", "pub use other_crate::{Type, ty};\n"),
                ("src/user.rs", "use crate::prelude::{Type, ty};"),
            ],
        );
        assert_eq!(
            ws.sorted_targets("test_crate::user"),
            [
                ("other_crate", "ty", None),
                ("other_crate", "ty", Some("Type"))
            ]
        );
    }

    // The same through `extern crate other_crate as oc;` in the root.
    #[test]
    fn reexport_through_an_extern_crate_alias_targets_the_other_root() {
        let ws = with_other_root_glob(
            "extern crate other_crate as oc;\nmod user;\npub use oc::{Type, ty};\n",
            &[("src/user.rs", "use crate::{Type, ty};")],
        );
        assert_eq!(
            ws.sorted_targets("test_crate::user"),
            [
                ("other_crate", "ty", None),
                ("other_crate", "ty", Some("Type"))
            ]
        );
    }

    // A child module named like a workspace crate shadows that crate's root.
    #[test]
    fn reexport_through_a_child_module_named_like_a_crate_stays_local() {
        let ws = TestWorkspace::new()
            .package("other_crate", &[("src/lib.rs", "pub struct Cmd;")])
            .package(
                "test_crate",
                &[
                    ("src/lib.rs", "pub mod cmds;\nmod user;\n"),
                    (
                        "src/cmds/mod.rs",
                        "mod other_crate;\npub use other_crate::Cmd;\n",
                    ),
                    ("src/cmds/other_crate.rs", "pub struct Cmd;\n"),
                    ("src/user.rs", "use crate::cmds::Cmd;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("test_crate::user"),
            [("test_crate", "cmds::other_crate", Some("Cmd"))]
        );
    }

    // A name a module republishes through `pub use other_crate::m::*` reaches
    // the module of the other crate that defines it.
    #[test]
    fn glob_reexport_from_a_module_of_another_crate_reaches_that_module() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod math;\n"),
                    ("src/math.rs", "pub fn align_to() {}\n"),
                ],
            )
            .package(
                "test_crate",
                &[
                    ("src/lib.rs", "pub mod util;\nmod user;\n"),
                    ("src/util.rs", "pub use other_crate::math::*;\n"),
                    ("src/user.rs", "use crate::util::align_to;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("test_crate::user"),
            [("other_crate", "math", Some("align_to"))]
        );
    }

    // Two crates that republish each other's modules by glob: a name behind
    // the cycle is still found, and a name nobody defines ends the chain.
    #[test]
    fn globs_republishing_each_other_across_crates_end() {
        let ws = TestWorkspace::new()
            .package(
                "test_crate",
                &[
                    ("src/lib.rs", "pub mod defs;\npub mod front;\nmod user;\n"),
                    ("src/front.rs", "pub use other_crate::back::*;\n"),
                    ("src/defs.rs", "pub fn deep() {}\n"),
                    ("src/user.rs", "use crate::front::{deep, missing};"),
                ],
            )
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod back;\n"),
                    (
                        "src/back.rs",
                        "pub use test_crate::front::*;\npub use test_crate::defs::*;\n",
                    ),
                ],
            )
            .analyze();
        assert_eq!(
            ws.sorted_targets("test_crate::user"),
            [
                ("test_crate", "defs", Some("deep")),
                ("test_crate", "front", Some("missing")),
            ]
        );
    }

    // A name a descendant takes through a private `use other_crate::m::*` of
    // its ancestor reaches the module of the other crate, not a module of
    // the same name in its own crate.
    #[test]
    fn private_glob_from_a_module_of_another_crate_reaches_that_module() {
        let ws = TestWorkspace::new()
            .package(
                "other_crate",
                &[
                    ("src/lib.rs", "pub mod math;\n"),
                    ("src/math.rs", "pub fn align_to() {}\n"),
                ],
            )
            .package(
                "test_crate",
                &[
                    ("src/lib.rs", "pub mod math;\npub mod util;\n"),
                    ("src/math.rs", "pub fn align_to() {}\n"),
                    ("src/util/mod.rs", "mod child;\nuse other_crate::math::*;\n"),
                    ("src/util/child.rs", "use super::align_to;"),
                ],
            )
            .analyze();
        assert_eq!(
            ws.targets("test_crate::util::child"),
            [("other_crate", "math", Some("align_to"))]
        );
    }

    // With test code, a root whose test build loads other files than its
    // build without `test` gets the modules of its test build beside them.
    #[test]
    fn test_build_modules_hold_the_files_a_test_build_loads() {
        let files = [
            (
                "src/lib.rs",
                "pub mod other;\n#[cfg(test)]\nmod fake;\npub mod alt;\nmod user;\n",
            ),
            ("src/other.rs", "pub fn helper() {}"),
            ("src/fake.rs", "pub use crate::other::helper;"),
            (
                "src/alt.rs",
                "#[cfg_attr(test, path = \"alt_test.rs\")]\npub mod inner;",
            ),
            ("src/alt/inner.rs", "pub fn real() {}"),
            ("src/alt_test.rs", "pub fn mock() {}"),
            (
                "src/user.rs",
                "#[cfg(test)]\nfn t() { crate::fake::helper(); crate::alt::inner::mock(); }",
            ),
        ];
        let with_tests = TestWorkspace::new()
            .package("test_crate", &files)
            .analyze_with_tests();
        let without_tests = TestWorkspace::new().package("test_crate", &files).analyze();

        assert_eq!(
            with_tests.sorted_targets("test_crate::user"),
            [
                ("test_crate", "alt::inner", Some("mock")),
                ("test_crate", "other", Some("helper")),
            ]
        );
        assert_eq!(
            defined_names(&with_tests, "test_crate::alt::inner"),
            ["mock", "real"]
        );
        assert_eq!(
            defined_names(&without_tests, "test_crate::alt::inner"),
            ["real"]
        );
        assert!(without_tests.deps("test_crate::user").is_empty());
    }

    // Two bindings of one name, each in builds of its own `cfg` or in a
    // namespace of its own, both stay: neither replaces the other.
    #[test]
    fn two_bindings_of_one_name_both_stay() {
        let tmp = TempDir::new().unwrap();
        for (path, content) in [
            (
                "src/lib.rs",
                "mod x;\nmod y;\n\
                 #[cfg(feature = \"a\")]\nuse crate::x::f;\n\
                 #[cfg(not(feature = \"a\"))]\nuse crate::y::f;\n\
                 #[cfg(feature = \"a\")]\npub use crate::x::g;\n\
                 #[cfg(not(feature = \"a\"))]\npub use crate::y::g;\n\
                 pub struct S {}\n\
                 #[allow(non_snake_case)]\npub fn S() {}\n",
            ),
            ("src/x.rs", "pub fn f() {}\npub fn g() {}\n"),
            ("src/y.rs", "pub fn f() {}\npub fn g() {}\n"),
        ] {
            let full = tmp.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
        }
        let crate_info = conventional_package("test_crate", tmp.path());
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["x".into(), "y".into()]),
        )]
        .into_iter()
        .collect();

        let mut roots = collect_crate_reexports(&crate_info, &mp, false);
        assert_eq!(roots.len(), 1, "expected one crate root");
        let result = roots.remove(0).modules;

        let root = &result[""];
        let counts = ["f", "g", "S"].map(|name| (name, root.named(name).count()));
        assert_eq!(counts, [("f", 2), ("g", 2), ("S", 2)], "{result:?}");
    }
}
