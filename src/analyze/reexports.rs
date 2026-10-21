//! Re-export collection for accurate dependency edges.
//!
//! Collects `pub use` re-exports and public definitions per module so that
//! downstream resolution can follow re-export chains to the original definition.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use syn::spanned::Spanned;

use super::packages::{ModulePathMap, PackageExportMap, WorkspacePackages, normalize_crate_name};
use crate::model::{DefKind, Definition, EdgeContext, PackageInfo};

use super::mod_resolver::{child_resolve_dir, extract_mod_declarations, resolve_mod_path};
use super::use_parser::{
    AssociatedItems, ModuleExportInfo, ReExportMap, ReExportTarget, ResolutionContext,
    RootReExports, is_reexport_visibility, resolve_single_path, resolve_use_tree,
};

/// Invariant parameters shared across the recursive re-export walk.
struct CollectContext<'a> {
    crate_name: &'a str,
    workspace_root: &'a Path,
    all_module_paths: &'a ModulePathMap,
    workspace_crates: &'a WorkspacePackages,
    crate_exports: &'a PackageExportMap,
    extern_crates: &'a HashMap<String, String>,
}

/// Collect re-exports and public definitions for all modules of a package,
/// one map per entry file, in `entry_files` order.
pub(crate) fn collect_crate_reexports(
    crate_info: &PackageInfo,
    all_module_paths: &ModulePathMap,
    workspace_crates: &WorkspacePackages,
    crate_exports: &PackageExportMap,
) -> Vec<RootReExports> {
    let crate_name = normalize_crate_name(&crate_info.name);

    crate_info
        .entry_files
        .iter()
        .map(|entry| {
            let extern_crates = root_extern_crates(&entry.file, workspace_crates);
            let ctx = CollectContext {
                crate_name: &crate_name,
                workspace_root: &crate_info.workspace_root,
                all_module_paths,
                workspace_crates,
                crate_exports,
                extern_crates: &extern_crates,
            };
            RootReExports {
                kind: entry.kind,
                modules: collect_root_reexports(&ctx, &entry.file),
            }
        })
        .collect()
}

/// Alias -> workspace crate name for each `extern crate X as Y;` in a crate
/// root, where `X` is a workspace crate. Empty when the file does not parse.
pub(crate) fn root_extern_crates(
    root_file: &Path,
    workspace_crates: &WorkspacePackages,
) -> HashMap<String, String> {
    let Ok(source) = std::fs::read_to_string(root_file) else {
        return HashMap::new();
    };
    let Ok(syntax) = syn::parse_file(&source) else {
        return HashMap::new();
    };
    syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::ExternCrate(extern_crate) => {
                let (_, alias) = extern_crate.rename.as_ref()?;
                let name = extern_crate.ident.to_string();
                workspace_crates
                    .contains(&name)
                    .then(|| (alias.to_string(), normalize_crate_name(&name)))
            }
            _ => None,
        })
        .collect()
}

fn collect_root_reexports(
    ctx: &CollectContext,
    root_file: &Path,
) -> HashMap<String, ModuleExportInfo> {
    let mut result = HashMap::new();
    let mut out_of_module_impls = Vec::new();
    walk_collect_reexports(ctx, root_file, "", &mut result, &mut out_of_module_impls);

    for block in out_of_module_impls {
        result
            .entry(block.module)
            .or_default()
            .associated
            .entry(block.type_name)
            .or_default()
            .merge(block.items);
    }

    result
}

/// An `impl` block written in another module than the one defining its type.
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

/// Extract re-exports, definitions and associated items from a single parsed
/// module file. An `impl` block whose type lives in another module goes to
/// `out_of_module_impls` for that module.
fn collect_module_info(
    ctx: &CollectContext,
    syntax: &syn::File,
    source_file: &Path,
    module_path: &str,
    out_of_module_impls: &mut Vec<OutOfModuleImpl>,
) -> ModuleExportInfo {
    let mut info = ModuleExportInfo::default();

    for item in &syntax.items {
        let defined = match item {
            syn::Item::Use(use_item) if is_reexport_visibility(&use_item.vis) => {
                collect_use_reexports(ctx, use_item, source_file, module_path, &mut info, false);
                None
            }
            syn::Item::Use(use_item) if matches!(use_item.vis, syn::Visibility::Inherited) => {
                collect_use_reexports(ctx, use_item, source_file, module_path, &mut info, true);
                None
            }
            syn::Item::Fn(i) => Some((&i.vis, &i.sig.ident, DefKind::Fn)),
            syn::Item::Struct(i) => Some((&i.vis, &i.ident, DefKind::Struct)),
            syn::Item::Enum(i) => Some((&i.vis, &i.ident, DefKind::Enum)),
            syn::Item::Trait(i) => Some((&i.vis, &i.ident, DefKind::Trait)),
            syn::Item::Const(i) => Some((&i.vis, &i.ident, DefKind::Const)),
            syn::Item::Static(i) => Some((&i.vis, &i.ident, DefKind::Static)),
            syn::Item::Type(i) => Some((&i.vis, &i.ident, DefKind::Type)),
            _ => None,
        };
        match defined {
            Some((vis, ident, kind)) if is_reexport_visibility(vis) => {
                let line = item.span().start().line;
                info.definitions
                    .insert(ident.to_string(), Definition { kind, line });
            }
            Some((_, ident, _)) => {
                info.private_definitions.insert(ident.to_string());
            }
            None => {}
        }
    }

    // Second pass, once every `use` of the file is known: an `impl` block may
    // precede the `use` that names its type.
    for item in &syntax.items {
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
                    impl_owner(ctx, &self_type.path, source_file, module_path, &info)
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

/// Find the module that defines the type an `impl` block is for, and the
/// type's name there. A bare name is this module's when it defines the name,
/// and otherwise whatever a `use` of this file binds it to; a qualified path
/// resolves like any other. What resolves to neither is no type of this crate,
/// and its `impl` is skipped.
fn impl_owner(
    ctx: &CollectContext,
    path: &syn::Path,
    source_file: &Path,
    module_path: &str,
    info: &ModuleExportInfo,
) -> Option<(ImplOwner, String)> {
    let name = path.segments.last()?.ident.to_string();
    if path.segments.len() == 1 {
        if info.definitions.contains_key(&name) {
            return Some((ImplOwner::ThisModule, name));
        }
        let bound = info
            .private_uses
            .get(&name)
            .or_else(|| info.explicit_reexports.get(&name))?;
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
    let dep = resolve_single_path(
        &resolution_context(ctx, source_file, module_path),
        &path_str,
        0,
        &EdgeContext::production(),
        0,
    )?;
    if dep.target_package != ctx.crate_name {
        return None;
    }
    let item = dep.target_item?;
    let owner = if dep.target_module == module_path {
        ImplOwner::ThisModule
    } else {
        ImplOwner::Other(dep.target_module)
    };
    Some((owner, item))
}

/// Build the resolution context of this phase: the re-export map is what is
/// being built, so it is empty here, and no external crate is known.
fn resolution_context<'a>(
    ctx: &'a CollectContext,
    source_file: &'a Path,
    module_path: &'a str,
) -> ResolutionContext<'a> {
    static EMPTY_REEXPORT_MAP: LazyLock<ReExportMap> = LazyLock::new(ReExportMap::default);
    static EMPTY_EXT_NAMES: LazyLock<HashMap<String, String>> = LazyLock::new(HashMap::new);
    ResolutionContext {
        current_crate: ctx.crate_name,
        workspace_crates: ctx.workspace_crates,
        source_file,
        all_module_paths: ctx.all_module_paths,
        crate_exports: ctx.crate_exports,
        current_module_path: module_path,
        reexport_map: &EMPTY_REEXPORT_MAP,
        external_crate_names: &EMPTY_EXT_NAMES,
        extern_crates: ctx.extern_crates,
        passed_on: None,
    }
}

/// Resolve a single `use` item into re-export entries.
///
/// `private` selects where named targets land: a `pub`/`pub(crate)`/`pub(super)`
/// re-export fills `explicit_reexports`, a private `use` fills `private_uses`
/// (a binding descendants can still name through this module). A glob lands in
/// `glob_sources` or `private_glob_sources` by the same split, a public one
/// from a module of another workspace crate in `foreign_glob_sources`. A private,
/// non-glob leaf the resolution chain cannot place at all names something
/// outside this crate; recording it in `elsewhere` keeps a descendant's
/// reference to it from resolving to this module.
fn collect_use_reexports(
    ctx: &CollectContext,
    use_item: &syn::ItemUse,
    source_file: &Path,
    module_path: &str,
    info: &mut ModuleExportInfo,
    private: bool,
) {
    let alias_paths = resolve_use_tree(&use_item.tree, "", true);
    let original_paths = resolve_use_tree(&use_item.tree, "", false);

    let res_ctx = resolution_context(ctx, source_file, module_path);

    for (alias_path, original_path) in alias_paths.iter().zip(original_paths.iter()) {
        let Some(mut dep) =
            resolve_single_path(&res_ctx, original_path, 0, &EdgeContext::production(), 0)
        else {
            let alias_name = alias_path.rsplit("::").next().unwrap_or(alias_path);
            if private && alias_name != "*" {
                info.elsewhere.insert(alias_name.to_string());
            }
            continue;
        };
        if let Some((crate_name, name)) = root_item_name(ctx, original_path)
            && dep.target_package == crate_name
        {
            dep.target_module = String::new();
            dep.target_item = Some(name.to_string());
        }
        // A module: its parent and its name. A whole workspace crate: its root,
        // which has neither. Without an item the resolution may also have
        // fallen back to a module that does not exist.
        let names_module = if dep.target_module.is_empty() {
            dep.target_package != ctx.crate_name
        } else {
            ctx.all_module_paths
                .get_or_empty(&dep.target_package)
                .contains(&dep.target_module)
        };
        if dep.target_item.is_none() && names_module {
            let (parent, name) = match dep.target_module.rsplit_once("::") {
                Some((parent, name)) => (parent.to_string(), name.to_string()),
                None => (String::new(), dep.target_module.clone()),
            };
            dep.target_module = parent;
            dep.target_item = Some(name);
        }

        if dep.target_item.as_deref() == Some("*") {
            if private {
                info.private_glob_sources.push(dep.target_module.clone());
            } else if dep.target_package != ctx.crate_name {
                info.foreign_glob_sources
                    .push((dep.target_package.clone(), dep.target_module.clone()));
            } else {
                info.glob_sources.push(dep.target_module.clone());
            }
        } else if let Some(original_name) = &dep.target_item {
            let alias_name = alias_path.rsplit("::").next().unwrap_or(alias_path);
            let target = ReExportTarget {
                crate_name: dep.target_package.clone(),
                module: dep.target_module.clone(),
                original_name: original_name.clone(),
            };
            if private {
                info.private_uses.insert(alias_name.to_string(), target);
            } else {
                info.explicit_reexports
                    .insert(alias_name.to_string(), target);
            }
        }
    }
}

/// The crate and item that `crate::Name`, `other_crate::Name` or `alias::Name`
/// (`extern crate other_crate as alias;`) names in a crate root, when `Name` is
/// no module. The resolution cannot tell while the
/// map is built: a name the root only re-exports is not in it yet.
fn root_item_name<'p>(ctx: &CollectContext, path: &'p str) -> Option<(String, &'p str)> {
    let (first, name) = path.split_once("::")?;
    let crate_name = if first == "crate" {
        ctx.crate_name.to_string()
    } else if ctx.workspace_crates.contains(first) {
        normalize_crate_name(first)
    } else {
        ctx.extern_crates.get(first)?.clone()
    };
    let is_item = !name.contains("::")
        && name != "*"
        && !ctx
            .all_module_paths
            .get_or_empty(&crate_name)
            .contains(name);
    is_item.then_some((crate_name, name))
}

fn walk_collect_reexports(
    ctx: &CollectContext,
    file_path: &Path,
    module_path: &str,
    result: &mut HashMap<String, ModuleExportInfo>,
    out_of_module_impls: &mut Vec<OutOfModuleImpl>,
) {
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

    let source_file = file_path
        .strip_prefix(ctx.workspace_root)
        .map_or_else(|_| file_path.to_path_buf(), Path::to_path_buf);

    let info = collect_module_info(ctx, &syntax, &source_file, module_path, out_of_module_impls);
    if !info.is_empty() {
        result.insert(module_path.to_string(), info);
    }

    // Recurse into child modules
    let decls = extract_mod_declarations(&syntax, false);
    let resolve_dir = child_resolve_dir(file_path, module_path.is_empty());

    for decl in decls {
        let child_file = if let Some(ref explicit) = decl.explicit_path {
            let p = resolve_dir.join(explicit);
            if p.exists() { Some(p) } else { None }
        } else {
            resolve_mod_path(&resolve_dir, &decl.name)
        };

        let child_module_path = if module_path.is_empty() {
            decl.name.clone()
        } else {
            format!("{module_path}::{}", decl.name)
        };

        if let Some(child_path) = child_file {
            walk_collect_reexports(
                ctx,
                &child_path,
                &child_module_path,
                result,
                out_of_module_impls,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::use_parser::resolve_reexport;
    use super::*;
    use crate::model::{DependencyRef, PackageInfo};
    use crate::test_support::conventional_package;
    use std::collections::HashSet;
    use tempfile::TempDir;

    /// The re-exports of a package with a single crate root.
    fn collect_single_root_reexports(
        crate_info: &PackageInfo,
        all_module_paths: &ModulePathMap,
        workspace_crates: &WorkspacePackages,
        crate_exports: &PackageExportMap,
    ) -> HashMap<String, ModuleExportInfo> {
        let mut roots = collect_crate_reexports(
            crate_info,
            all_module_paths,
            workspace_crates,
            crate_exports,
        );
        assert_eq!(roots.len(), 1, "expected one crate root");
        roots.remove(0).modules
    }

    fn test_crate(files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (path, content) in files {
            let full = tmp.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, content).unwrap();
        }
        tmp
    }

    fn make_crate_info(tmp: &TempDir, name: &str) -> PackageInfo {
        conventional_package(name, tmp.path())
    }

    // --- Cycles 5-12: collect_crate_reexports ---

    // Cycle 5: pub use sibling::Item
    #[test]
    fn collects_pub_use_sibling_item() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod parent;"),
            (
                "src/parent/mod.rs",
                "pub mod sibling;\npub use sibling::Item;",
            ),
            ("src/parent/sibling.rs", "pub struct Item;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let parent_info = result.get("parent").expect("parent should have exports");
        let reexport = parent_info
            .explicit_reexports
            .get("Item")
            .expect("Item should be re-exported");
        assert_eq!(reexport.module, "parent::sibling");
        assert_eq!(reexport.original_name, "Item");
    }

    // Cycle 6: pub use crate::model::Config
    #[test]
    fn collects_pub_use_crate_module_item() {
        let tmp = test_crate(&[
            (
                "src/lib.rs",
                "pub mod model;\npub use crate::model::Config;",
            ),
            ("src/model.rs", "pub struct Config;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [("test_crate".to_string(), HashSet::from(["model".into()]))]
            .into_iter()
            .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let root_info = result.get("").expect("root should have exports");
        let reexport = root_info
            .explicit_reexports
            .get("Config")
            .expect("Config should be re-exported");
        assert_eq!(reexport.module, "model");
        assert_eq!(reexport.original_name, "Config");
    }

    // Cycle 7: pub use super::sibling::Item
    #[test]
    fn collects_pub_use_super_sibling_item() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod parent;"),
            ("src/parent/mod.rs", "pub mod sibling;\npub mod child;"),
            ("src/parent/sibling.rs", "pub struct Item;"),
            ("src/parent/child.rs", "pub use super::sibling::Item;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from([
                "parent".into(),
                "parent::sibling".into(),
                "parent::child".into(),
            ]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let child_info = result
            .get("parent::child")
            .expect("child should have exports");
        let reexport = child_info
            .explicit_reexports
            .get("Item")
            .expect("Item should be re-exported");
        assert_eq!(reexport.module, "parent::sibling");
        assert_eq!(reexport.original_name, "Item");
    }

    // Cycle 8: pub use elements::*
    #[test]
    fn collects_pub_use_glob() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod render;"),
            (
                "src/render/mod.rs",
                "pub mod elements;\npub use elements::*;",
            ),
            ("src/render/elements.rs", "pub struct Widget;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["render".into(), "render::elements".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let render_info = result.get("render").expect("render should have exports");
        assert!(
            render_info
                .glob_sources
                .contains(&"render::elements".to_string()),
            "glob_sources should contain render::elements, found: {:?}",
            render_info.glob_sources
        );
    }

    // Cycle 9: pub use sibling::{A, B}
    #[test]
    fn collects_pub_use_group_import() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod parent;"),
            (
                "src/parent/mod.rs",
                "pub mod sibling;\npub use sibling::{Alpha, Beta};",
            ),
            (
                "src/parent/sibling.rs",
                "pub struct Alpha;\npub struct Beta;",
            ),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let parent_info = result.get("parent").expect("parent should have exports");
        assert!(
            parent_info.explicit_reexports.contains_key("Alpha"),
            "missing Alpha"
        );
        assert!(
            parent_info.explicit_reexports.contains_key("Beta"),
            "missing Beta"
        );
        assert_eq!(
            parent_info.explicit_reexports["Alpha"].module,
            "parent::sibling"
        );
        assert_eq!(
            parent_info.explicit_reexports["Beta"].module,
            "parent::sibling"
        );
    }

    // Cycle 10: pub(crate) use captured, private use NOT
    #[test]
    fn pub_crate_use_captured_private_not() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod parent;"),
            (
                "src/parent/mod.rs",
                "pub mod sibling;\npub(crate) use sibling::Public;\nuse sibling::Private;",
            ),
            (
                "src/parent/sibling.rs",
                "pub struct Public;\npub struct Private;",
            ),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let parent_info = result.get("parent").expect("parent should have exports");
        assert!(
            parent_info.explicit_reexports.contains_key("Public"),
            "pub(crate) use should be captured"
        );
        assert!(
            !parent_info.explicit_reexports.contains_key("Private"),
            "private use should NOT be captured"
        );
    }

    // A private use lands in private_uses, where a descendant can still resolve it
    #[test]
    fn private_use_captured_in_private_uses() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod parent;"),
            (
                "src/parent/mod.rs",
                "pub mod sibling;\nuse sibling::Private;",
            ),
            ("src/parent/sibling.rs", "pub struct Private;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let parent_info = result.get("parent").expect("parent should have exports");
        let target = parent_info
            .private_uses
            .get("Private")
            .expect("private use should be captured in private_uses");
        assert_eq!(target.module, "parent::sibling");
        assert_eq!(target.original_name, "Private");
        assert!(
            !parent_info.explicit_reexports.contains_key("Private"),
            "private use must not leak into explicit_reexports"
        );
    }

    // A private glob lands in private_glob_sources, where a descendant can
    // still resolve the names it forwards
    #[test]
    fn private_glob_captured_in_private_glob_sources() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod parent;"),
            ("src/parent/mod.rs", "pub mod sibling;\nuse sibling::*;"),
            ("src/parent/sibling.rs", "pub struct Private;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let parent_info = result.get("parent").expect("parent should have exports");
        assert_eq!(
            parent_info.private_glob_sources,
            vec!["parent::sibling".to_string()]
        );
        assert!(
            parent_info.glob_sources.is_empty(),
            "private glob must not leak into glob_sources"
        );
    }

    // Cycle 11: pub struct Foo → definitions
    #[test]
    fn collects_pub_struct_as_definition() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod module;"),
            ("src/module.rs", "pub struct Foo;\npub fn helper() {}"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [("test_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let module_info = result.get("module").expect("module should have exports");
        assert!(
            module_info.definitions.contains_key("Foo"),
            "should contain Foo"
        );
        assert!(
            module_info.definitions.contains_key("helper"),
            "should contain helper"
        );
    }

    /// Characterization: which item kinds reach `definitions`, and with which
    /// visibilities. Pins the pre-`DefKind` behaviour.
    #[test]
    fn definitions_cover_all_matched_item_kinds() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod module;"),
            (
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
            ),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [("test_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let defs = &result.get("module").expect("module info").definitions;
        for (name, kind) in [
            ("a_fn", DefKind::Fn),
            ("AStruct", DefKind::Struct),
            ("AnEnum", DefKind::Enum),
            ("ATrait", DefKind::Trait),
            ("A_CONST", DefKind::Const),
            ("A_STATIC", DefKind::Static),
            ("AnAlias", DefKind::Type),
            ("CrateStruct", DefKind::Struct),
            ("SuperStruct", DefKind::Struct),
        ] {
            assert_eq!(
                defs.get(name).map(|d| d.kind),
                Some(kind),
                "wrong kind for {name}"
            );
        }
        for name in [
            "AUnion",
            "private_fn",
            "PrivateStruct",
            "InPathStruct",
            "method",
            "V",
        ] {
            assert!(!defs.contains_key(name), "unexpected {name} in {defs:?}");
        }
    }

    /// A private item in the root is visible to every module of the crate,
    /// but not to other crates, so it stays out of `definitions`.
    #[test]
    fn private_root_items_are_recorded_apart_from_definitions() {
        let tmp = test_crate(&[
            (
                "src/lib.rs",
                "pub mod module;\nstruct Hidden;\nfn helper() {}\npub struct Shown;\n",
            ),
            ("src/module.rs", ""),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [("test_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let root = result.get("").expect("root info");
        assert_eq!(
            root.private_definitions,
            HashSet::from(["Hidden".to_string(), "helper".to_string()])
        );
        assert!(!root.definitions.contains_key("Hidden"));
        assert!(root.definitions.contains_key("Shown"));
    }

    /// `extern crate other_crate as oc;` in the root lets the root re-export
    /// from `oc`, which names `other_crate`.
    #[test]
    fn root_reexport_through_extern_crate_alias() {
        let tmp = test_crate(&[(
            "src/lib.rs",
            "extern crate other_crate as oc;\npub use oc::thing::Thing;\n",
        )]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let ws: WorkspacePackages = ["test_crate", "other_crate"].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["thing".into()]))]
            .into_iter()
            .collect();

        let result =
            collect_single_root_reexports(&crate_info, &mp, &ws, &PackageExportMap::default());

        let target = result
            .get("")
            .and_then(|root| root.explicit_reexports.get("Thing"));
        assert_eq!(
            target.map(|t| (
                t.crate_name.as_str(),
                t.module.as_str(),
                t.original_name.as_str()
            )),
            Some(("other_crate", "thing", "Thing")),
            "{result:?}"
        );
    }

    /// `pub(self)` and `pub(in …)` in the root reach no further than the
    /// crate, like a private item.
    #[test]
    fn restricted_root_items_are_recorded_as_private() {
        let tmp = test_crate(&[
            (
                "src/lib.rs",
                "pub mod module;\npub(self) struct SelfScoped;\npub(in crate) fn in_crate() {}\n",
            ),
            ("src/module.rs", ""),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [("test_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let root = result.get("").expect("root info");
        assert_eq!(
            root.private_definitions,
            HashSet::from(["SelfScoped".to_string(), "in_crate".to_string()])
        );
        assert!(root.definitions.is_empty());
    }

    /// Each definition records the line its item starts on, attributes
    /// included, so a jump lands on the item as the reader sees it.
    #[test]
    fn definitions_carry_the_line_of_the_defining_item() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod module;"),
            (
                "src/module.rs",
                "//! Module doc\n\npub fn a_fn() {}\n\n#[derive(Debug)]\npub struct AStruct;\n",
            ),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [("test_crate".to_string(), HashSet::from(["module".into()]))]
            .into_iter()
            .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let defs = &result.get("module").expect("module info").definitions;
        assert_eq!(defs.get("a_fn").map(|d| d.line), Some(3));
        assert_eq!(defs.get("AStruct").map(|d| d.line), Some(5));
    }

    /// Characterization: `definitions` is top-level only — items inside an
    /// inline `mod { … }` body are not collected for any module path.
    #[test]
    fn definitions_skip_inline_mod_bodies() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod module;"),
            (
                "src/module.rs",
                "pub struct Outer;\npub mod inner { pub struct Inner; }",
            ),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["module".into(), "module::inner".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let module_info = result.get("module").expect("module info");
        assert!(module_info.definitions.contains_key("Outer"));
        assert!(!module_info.definitions.contains_key("Inner"));
        assert!(
            !result.contains_key("module::inner"),
            "inline mod produces no entry, found: {:?}",
            result.get("module::inner")
        );
    }

    // Cycle 12: pub use sibling::Item as Widget (rename)
    #[test]
    fn collects_pub_use_rename() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod parent;"),
            (
                "src/parent/mod.rs",
                "pub mod sibling;\npub use sibling::Item as Widget;",
            ),
            ("src/parent/sibling.rs", "pub struct Item;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["parent".into(), "parent::sibling".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let parent_info = result.get("parent").expect("parent should have exports");
        assert!(
            !parent_info.explicit_reexports.contains_key("Item"),
            "should NOT have original name as key"
        );
        let reexport = parent_info
            .explicit_reexports
            .get("Widget")
            .expect("Widget should be the re-export key");
        assert_eq!(reexport.module, "parent::sibling");
        assert_eq!(reexport.original_name, "Item");
    }

    /// Variants, `impl` fns and consts, and trait fns are recorded on the module
    /// that defines the type. An `impl` block in a sibling file lands there too.
    #[test]
    fn collects_associated_items_on_the_defining_module() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod back;"),
            ("src/back/mod.rs", "pub mod writer;\npub mod features;"),
            (
                "src/back/writer.rs",
                "pub enum Mode { Fast, Slow(u8) }\n\
                 pub struct Writer;\n\
                 impl Writer { pub fn new() -> Self { Writer } pub const MAX: u8 = 1; }\n\
                 pub trait Step { fn run(&self); }\n\
                 impl Step for Writer { fn run(&self) {} }",
            ),
            (
                "src/back/features.rs",
                "use super::writer::Writer;\nimpl Writer { pub fn write_feature(&self) {} }",
            ),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from([
                "back".into(),
                "back::writer".into(),
                "back::features".into(),
            ]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let writer = &result["back::writer"].associated;
        assert_eq!(
            writer["Mode"].variants,
            HashSet::from(["Fast".into(), "Slow".into()])
        );
        assert_eq!(
            writer["Writer"].fns,
            HashSet::from(["new".into(), "run".into(), "write_feature".into()]),
            "own impl, trait impl and the sibling file's impl"
        );
        assert_eq!(writer["Writer"].consts, HashSet::from(["MAX".into()]));
        assert_eq!(writer["Step"].fns, HashSet::from(["run".into()]));
        assert!(
            !result
                .get("back::features")
                .is_some_and(|f| f.associated.contains_key("Writer")),
            "the impl in features is Writer's, not a type of features"
        );
    }

    // A name the root only re-exports is not known while the map is built, so
    // a `use crate::Name` records the root as its target and the resolution
    // follows the root's re-export later.
    #[test]
    fn use_of_a_name_the_root_reexports_targets_the_root() {
        let tmp = test_crate(&[
            (
                "src/lib.rs",
                "pub mod span;\npub mod value;\npub mod front;\n\
                 pub use crate::span::Span;\npub use value::*;",
            ),
            ("src/span.rs", "pub struct Span;"),
            ("src/value.rs", "pub struct Value;"),
            ("src/front/mod.rs", "pub mod glsl;\npub mod prelude;"),
            ("src/front/glsl/mod.rs", "use crate::Span;"),
            ("src/front/prelude.rs", "pub use crate::Value;"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from([
                "span".into(),
                "value".into(),
                "front".into(),
                "front::glsl".into(),
                "front::prelude".into(),
            ]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let private = result
            .get("front::glsl")
            .and_then(|info| info.private_uses.get("Span"))
            .expect("the private use of a root re-export is recorded");
        assert_eq!(private.module, "");
        assert_eq!(private.original_name, "Span");
        let public = result
            .get("front::prelude")
            .and_then(|info| info.explicit_reexports.get("Value"))
            .expect("the re-export of a root glob name is recorded");
        assert_eq!(public.module, "");
        assert_eq!(public.original_name, "Value");
    }

    // A `pub use` of a whole workspace crate re-exports that crate's root.
    #[test]
    fn reexport_of_a_workspace_crate_targets_its_root() {
        let tmp = test_crate(&[("src/lib.rs", "pub use other_crate;\n")]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let ws: WorkspacePackages = ["test_crate", "other_crate"].into_iter().collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &ModulePathMap::default(),
            &ws,
            &PackageExportMap::default(),
        );

        let target = result
            .get("")
            .and_then(|root| root.explicit_reexports.get("other_crate"));
        assert_eq!(
            target.map(|t| (
                t.crate_name.as_str(),
                t.module.as_str(),
                t.original_name.as_str()
            )),
            Some(("other_crate", "", "")),
            "{result:?}"
        );
    }

    // A name another crate's root only brings in through a glob is unknown
    // while the map is built; the re-export records that root as its target.
    #[test]
    fn reexport_of_a_name_another_root_reexports_targets_that_root() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod prelude;\n"),
            ("src/prelude.rs", "pub use other_crate::{Type, ty};\n"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let ws: WorkspacePackages = ["test_crate", "other_crate"].into_iter().collect();
        let mp: ModulePathMap = [
            ("test_crate".to_string(), HashSet::from(["prelude".into()])),
            ("other_crate".to_string(), HashSet::from(["ty".into()])),
        ]
        .into_iter()
        .collect();

        let result =
            collect_single_root_reexports(&crate_info, &mp, &ws, &PackageExportMap::default());

        let prelude = result.get("prelude").expect("prelude is collected");
        let target = |name: &str| {
            prelude.explicit_reexports.get(name).map(|t| {
                (
                    t.crate_name.as_str(),
                    t.module.as_str(),
                    t.original_name.as_str(),
                )
            })
        };
        assert_eq!(
            target("Type"),
            Some(("other_crate", "", "Type")),
            "{result:?}"
        );
        assert_eq!(target("ty"), Some(("other_crate", "", "ty")), "{result:?}");
    }

    // The same through `extern crate other_crate as oc;` in the root.
    #[test]
    fn reexport_through_an_extern_crate_alias_targets_the_other_root() {
        let tmp = test_crate(&[(
            "src/lib.rs",
            "extern crate other_crate as oc;\npub use oc::{Type, ty};\n",
        )]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let ws: WorkspacePackages = ["test_crate", "other_crate"].into_iter().collect();
        let mp: ModulePathMap = [("other_crate".to_string(), HashSet::from(["ty".into()]))]
            .into_iter()
            .collect();

        let result =
            collect_single_root_reexports(&crate_info, &mp, &ws, &PackageExportMap::default());

        let root = result.get("").expect("root is collected");
        let target = |name: &str| {
            root.explicit_reexports.get(name).map(|t| {
                (
                    t.crate_name.as_str(),
                    t.module.as_str(),
                    t.original_name.as_str(),
                )
            })
        };
        assert_eq!(
            target("Type"),
            Some(("other_crate", "", "Type")),
            "{result:?}"
        );
        assert_eq!(target("ty"), Some(("other_crate", "", "ty")), "{result:?}");
    }

    // A child module named like a workspace crate shadows that crate's root.
    #[test]
    fn reexport_through_a_child_module_named_like_a_crate_stays_local() {
        let tmp = test_crate(&[
            ("src/lib.rs", "pub mod cmds;\n"),
            (
                "src/cmds/mod.rs",
                "mod other_crate;\npub use other_crate::Cmd;\n",
            ),
            ("src/cmds/other_crate.rs", "pub struct Cmd;\n"),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let ws: WorkspacePackages = ["test_crate", "other_crate"].into_iter().collect();
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["cmds".into(), "cmds::other_crate".into()]),
        )]
        .into_iter()
        .collect();

        let result =
            collect_single_root_reexports(&crate_info, &mp, &ws, &PackageExportMap::default());

        let target = result
            .get("cmds")
            .and_then(|info| info.explicit_reexports.get("Cmd"))
            .map(|t| {
                (
                    t.crate_name.as_str(),
                    t.module.as_str(),
                    t.original_name.as_str(),
                )
            });
        assert_eq!(
            target,
            Some(("test_crate", "cmds::other_crate", "Cmd")),
            "{result:?}"
        );
    }

    // A name a module republishes through `pub use other_crate::m::*` reaches
    // the module of the other crate that defines it.
    #[test]
    fn glob_reexport_from_a_module_of_another_crate_reaches_that_module() {
        let ws: WorkspacePackages = ["test_crate", "other_crate"].into_iter().collect();
        let mp: ModulePathMap = [
            ("test_crate".to_string(), HashSet::from(["util".into()])),
            ("other_crate".to_string(), HashSet::from(["math".into()])),
        ]
        .into_iter()
        .collect();
        let user = test_crate(&[
            ("src/lib.rs", "pub mod util;\n"),
            ("src/util.rs", "pub use other_crate::math::*;\n"),
        ]);
        let other = test_crate(&[
            ("src/lib.rs", "pub mod math;\n"),
            ("src/math.rs", "pub fn align_to() {}\n"),
        ]);
        let collect = |tmp: &TempDir, name: &str| {
            let info = make_crate_info(tmp, name);
            collect_single_root_reexports(&info, &mp, &ws, &PackageExportMap::default())
        };
        let map: ReExportMap = [
            ("test_crate".to_string(), collect(&user, "test_crate")),
            ("other_crate".to_string(), collect(&other, "other_crate")),
        ]
        .into_iter()
        .collect();

        let mut dep = DependencyRef {
            target_package: "test_crate".to_string(),
            target_module: "util".to_string(),
            target_item: Some("align_to".to_string()),
            source_file: "src/main.rs".into(),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
            uses: Vec::new(),
            in_lib_view: false,
        };
        resolve_reexport(&mut dep, &map, "");

        assert_eq!(
            (dep.target_package.as_str(), dep.target_module.as_str()),
            ("other_crate", "math")
        );
    }

    // A `pub use` of a module re-exports it by its parent and its name.
    #[test]
    fn reexport_of_a_module_targets_it_in_its_parent() {
        let tmp = test_crate(&[
            ("src/lib.rs", "mod backend;\npub use backend::custom;\n"),
            ("src/backend/mod.rs", "pub mod custom;"),
            ("src/backend/custom.rs", ""),
        ]);
        let crate_info = make_crate_info(&tmp, "test_crate");
        let mp: ModulePathMap = [(
            "test_crate".to_string(),
            HashSet::from(["backend".into(), "backend::custom".into()]),
        )]
        .into_iter()
        .collect();

        let result = collect_single_root_reexports(
            &crate_info,
            &mp,
            &WorkspacePackages::default(),
            &PackageExportMap::default(),
        );

        let target = result
            .get("")
            .and_then(|root| root.explicit_reexports.get("custom"));
        assert_eq!(
            target.map(|t| (
                t.crate_name.as_str(),
                t.module.as_str(),
                t.original_name.as_str()
            )),
            Some(("test_crate", "backend", "custom")),
            "{result:?}"
        );
    }
}
