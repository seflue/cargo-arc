//! Re-export collection for accurate dependency edges.
//!
//! Collects `pub use` re-exports and public definitions per module so that
//! downstream resolution can follow re-export chains to the original definition.

use std::collections::HashMap;
use std::path::Path;

use crate::model::{
    CrateExportMap, CrateInfo, EdgeContext, ModulePathMap, WorkspaceCrates, normalize_crate_name,
};

use super::mod_resolver::{
    child_resolve_dir, extract_mod_declarations, find_crate_root_files, resolve_mod_path,
};
use super::use_parser::{
    ModuleExportInfo, ReExportMap, ReExportTarget, ResolutionContext, resolve_single_path,
    resolve_use_tree,
};

/// Check whether visibility qualifies as a re-export:
/// pub, pub(crate), pub(super) — but NOT private or pub(in path).
fn is_reexport_visibility(vis: &syn::Visibility) -> bool {
    match vis {
        syn::Visibility::Public(_) => true,
        syn::Visibility::Restricted(r) => {
            r.in_token.is_none() && (r.path.is_ident("crate") || r.path.is_ident("super"))
        }
        syn::Visibility::Inherited => false,
    }
}

/// Invariant parameters shared across the recursive re-export walk.
struct CollectContext<'a> {
    crate_name: &'a str,
    crate_root: &'a Path,
    all_module_paths: &'a ModulePathMap,
    workspace_crates: &'a WorkspaceCrates,
    crate_exports: &'a CrateExportMap,
}

/// Collect re-exports and public definitions for all modules in a crate.
pub(crate) fn collect_crate_reexports(
    crate_info: &CrateInfo,
    all_module_paths: &ModulePathMap,
    workspace_crates: &WorkspaceCrates,
    crate_exports: &CrateExportMap,
) -> HashMap<String, ModuleExportInfo> {
    let Ok(root_files) = find_crate_root_files(&crate_info.path) else {
        return HashMap::new();
    };
    let crate_name = normalize_crate_name(&crate_info.name);
    let ctx = CollectContext {
        crate_name: &crate_name,
        crate_root: &crate_info.path,
        all_module_paths,
        workspace_crates,
        crate_exports,
    };
    let mut result = HashMap::new();

    for root_file in root_files {
        walk_collect_reexports(&ctx, &root_file, "", &mut result);
    }

    result
}

/// Extract re-exports and definitions from a single parsed module file.
fn collect_module_info(
    ctx: &CollectContext,
    syntax: &syn::File,
    source_file: &Path,
    module_path: &str,
) -> ModuleExportInfo {
    let mut info = ModuleExportInfo::default();

    for item in &syntax.items {
        match item {
            syn::Item::Use(use_item) if is_reexport_visibility(&use_item.vis) => {
                collect_use_reexports(ctx, use_item, source_file, module_path, &mut info);
            }
            syn::Item::Fn(i) if is_reexport_visibility(&i.vis) => {
                info.definitions.insert(i.sig.ident.to_string());
            }
            syn::Item::Struct(i) if is_reexport_visibility(&i.vis) => {
                info.definitions.insert(i.ident.to_string());
            }
            syn::Item::Enum(i) if is_reexport_visibility(&i.vis) => {
                info.definitions.insert(i.ident.to_string());
            }
            syn::Item::Trait(i) if is_reexport_visibility(&i.vis) => {
                info.definitions.insert(i.ident.to_string());
            }
            syn::Item::Const(i) if is_reexport_visibility(&i.vis) => {
                info.definitions.insert(i.ident.to_string());
            }
            syn::Item::Static(i) if is_reexport_visibility(&i.vis) => {
                info.definitions.insert(i.ident.to_string());
            }
            syn::Item::Type(i) if is_reexport_visibility(&i.vis) => {
                info.definitions.insert(i.ident.to_string());
            }
            _ => {}
        }
    }

    info
}

/// Resolve a single `pub use` item into re-export entries.
fn collect_use_reexports(
    ctx: &CollectContext,
    use_item: &syn::ItemUse,
    source_file: &Path,
    module_path: &str,
    info: &mut ModuleExportInfo,
) {
    let alias_paths = resolve_use_tree(&use_item.tree, "", true);
    let original_paths = resolve_use_tree(&use_item.tree, "", false);

    let empty_reexport_map = ReExportMap::default();
    let empty_ext_names = std::collections::HashMap::new();
    let res_ctx = ResolutionContext {
        current_crate: ctx.crate_name,
        workspace_crates: ctx.workspace_crates,
        source_file,
        all_module_paths: ctx.all_module_paths,
        crate_exports: ctx.crate_exports,
        current_module_path: module_path,
        reexport_map: &empty_reexport_map,
        external_crate_names: &empty_ext_names,
    };

    for (alias_path, original_path) in alias_paths.iter().zip(original_paths.iter()) {
        let Some(dep) =
            resolve_single_path(&res_ctx, original_path, 0, &EdgeContext::production(), 0)
        else {
            continue;
        };

        if dep.target_item.as_deref() == Some("*") {
            info.glob_sources.push(dep.target_module.clone());
        } else if let Some(original_name) = &dep.target_item {
            let alias_name = alias_path.rsplit("::").next().unwrap_or(alias_path);
            info.explicit_reexports.insert(
                alias_name.to_string(),
                ReExportTarget {
                    module: dep.target_module.clone(),
                    original_name: original_name.clone(),
                },
            );
        }
    }
}

fn walk_collect_reexports(
    ctx: &CollectContext,
    file_path: &Path,
    module_path: &str,
    result: &mut HashMap<String, ModuleExportInfo>,
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
        .strip_prefix(ctx.crate_root)
        .map_or_else(|_| file_path.to_path_buf(), Path::to_path_buf);

    let info = collect_module_info(ctx, &syntax, &source_file, module_path);
    if !info.is_empty() {
        result.insert(module_path.to_string(), info);
    }

    // Recurse into child modules
    let decls = extract_mod_declarations(&syntax, false);
    let resolve_dir = child_resolve_dir(file_path);

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
            walk_collect_reexports(ctx, &child_path, &child_module_path, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CrateInfo;
    use std::collections::HashSet;
    use tempfile::TempDir;

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

    fn make_crate_info(tmp: &TempDir, name: &str) -> CrateInfo {
        CrateInfo {
            name: name.to_string(),
            path: tmp.path().to_path_buf(),
            dependencies: vec![],
            dev_dependencies: vec![],
        }
    }

    // --- Cycle 3: is_reexport_visibility ---

    fn parse_visibility(code: &str) -> syn::Visibility {
        let file: syn::File = syn::parse_str(code).unwrap();
        match &file.items[0] {
            syn::Item::Use(u) => u.vis.clone(),
            syn::Item::Struct(s) => s.vis.clone(),
            _ => panic!("expected use or struct item"),
        }
    }

    #[test]
    fn pub_is_reexport() {
        let vis = parse_visibility("pub use foo::Bar;");
        assert!(is_reexport_visibility(&vis));
    }

    #[test]
    fn pub_crate_is_reexport() {
        let vis = parse_visibility("pub(crate) use foo::Bar;");
        assert!(is_reexport_visibility(&vis));
    }

    #[test]
    fn pub_super_is_reexport() {
        let vis = parse_visibility("pub(super) use foo::Bar;");
        assert!(is_reexport_visibility(&vis));
    }

    #[test]
    fn private_is_not_reexport() {
        let vis = parse_visibility("use foo::Bar;");
        assert!(!is_reexport_visibility(&vis));
    }

    #[test]
    fn pub_in_path_is_not_reexport() {
        let vis = parse_visibility("pub(in crate::parent) use foo::Bar;");
        assert!(!is_reexport_visibility(&vis));
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
        );

        let module_info = result.get("module").expect("module should have exports");
        assert!(
            module_info.definitions.contains("Foo"),
            "should contain Foo"
        );
        assert!(
            module_info.definitions.contains("helper"),
            "should contain helper"
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

        let result = collect_crate_reexports(
            &crate_info,
            &mp,
            &WorkspaceCrates::default(),
            &CrateExportMap::default(),
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
}
