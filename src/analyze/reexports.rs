//! Re-export collection for accurate dependency edges.
//!
//! Collects `pub use` re-exports and public definitions per module so that
//! downstream resolution can follow re-export chains to the original definition.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::Path;

use crate::model::{
    CrateExportMap, CrateInfo, DependencyRef, EdgeContext, ModulePathMap, WorkspaceCrates,
    normalize_crate_name,
};

use super::syn_walker::{
    child_resolve_dir, extract_mod_declarations, find_crate_root_files, resolve_mod_path,
};
use super::use_parser::{ResolutionContext, resolve_single_path, resolve_use_tree};

/// Where a re-exported symbol originally comes from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReExportTarget {
    /// Source module path (crate-relative, e.g. `"render::elements"`)
    pub(crate) module: String,
    /// Original name in the source module (differs from map key on rename)
    pub(crate) original_name: String,
}

/// Export and re-export information for a single module.
#[derive(Debug, Default, Clone)]
pub(crate) struct ModuleExportInfo {
    /// Own public definitions (pub fn, pub struct, etc.)
    pub(crate) definitions: HashSet<String>,
    /// Explicit re-exports: alias/name → source target
    pub(crate) explicit_reexports: HashMap<String, ReExportTarget>,
    /// Glob re-export sources (module paths from `pub use *`)
    pub(crate) glob_sources: Vec<String>,
}

impl ModuleExportInfo {
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.definitions.is_empty()
            && self.explicit_reexports.is_empty()
            && self.glob_sources.is_empty()
    }
}

/// Crate name → (module path → export info).
/// Module paths are crate-relative (e.g. `"render"`, `"analyze::use_parser"`).
/// Empty string "" = crate root (lib.rs/main.rs).
#[derive(Debug, Default, Clone)]
pub(crate) struct ReExportMap(HashMap<String, HashMap<String, ModuleExportInfo>>);

impl Deref for ReExportMap {
    type Target = HashMap<String, HashMap<String, ModuleExportInfo>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<(String, HashMap<String, ModuleExportInfo>)> for ReExportMap {
    fn from_iter<I: IntoIterator<Item = (String, HashMap<String, ModuleExportInfo>)>>(
        iter: I,
    ) -> Self {
        Self(iter.into_iter().collect())
    }
}

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
    let res_ctx = ResolutionContext {
        current_crate: ctx.crate_name,
        workspace_crates: ctx.workspace_crates,
        source_file,
        all_module_paths: ctx.all_module_paths,
        crate_exports: ctx.crate_exports,
        current_module_path: module_path,
        reexport_map: &empty_reexport_map,
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

/// Resolve re-exports in a [`DependencyRef`]: follow the re-export chain
/// until the original definition module is found.
/// Modifies `dep.target_module` in place. No-op if no re-export applies.
pub(crate) fn resolve_reexport(dep: &mut DependencyRef, reexport_map: &ReExportMap) {
    let Some(crate_exports) = reexport_map.get(&dep.target_crate) else {
        return;
    };
    let mut visited = HashSet::new();
    let mut lookup_name = dep.target_item.clone();

    loop {
        if !visited.insert(dep.target_module.clone()) {
            break;
        }
        let Some(module_info) = crate_exports.get(&dep.target_module) else {
            break;
        };
        let Some(item) = &lookup_name else {
            break;
        };

        if module_info.definitions.contains(item) {
            break;
        }

        // Tier 1: Explicit re-export
        if let Some(target) = module_info.explicit_reexports.get(item) {
            let original_target = dep.target_module.clone();
            dep.target_module = target.module.clone();
            tracing::debug!(
                "re-export resolved: {} -> {} (via re-export in {})",
                original_target,
                dep.target_module,
                original_target
            );
            lookup_name = Some(target.original_name.clone());
            continue;
        }

        // Tier 2: Glob re-exports
        let mut found = false;
        for glob_src in &module_info.glob_sources {
            let mut glob_visited = HashSet::new();
            if module_exports_symbol(crate_exports, glob_src, item, &mut glob_visited) {
                let original_target = dep.target_module.clone();
                dep.target_module = glob_src.clone();
                tracing::debug!(
                    "re-export resolved: {} -> {} (via glob re-export in {})",
                    original_target,
                    dep.target_module,
                    original_target
                );
                found = true;
                break;
            }
        }
        if found {
            continue;
        }

        break;
    }
}

/// Check whether a module exports a symbol (own definition OR re-export).
fn module_exports_symbol(
    crate_exports: &HashMap<String, ModuleExportInfo>,
    module_path: &str,
    symbol: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(module_path.to_string()) {
        return false;
    }
    let Some(info) = crate_exports.get(module_path) else {
        return false;
    };
    if info.definitions.contains(symbol) {
        return true;
    }
    if info.explicit_reexports.contains_key(symbol) {
        return true;
    }
    for glob_src in &info.glob_sources {
        if module_exports_symbol(crate_exports, glob_src, symbol, visited) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CrateInfo;
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

    use std::path::PathBuf;

    fn test_dep(crate_name: &str, module: &str, item: Option<&str>) -> DependencyRef {
        DependencyRef {
            target_crate: crate_name.to_string(),
            target_module: module.to_string(),
            target_item: item.map(String::from),
            source_file: PathBuf::from("src/lib.rs"),
            line: 1,
            context: EdgeContext::production(),
        }
    }

    // --- resolve_reexport: Cycle 1 — No target_item → dep unchanged ---

    #[test]
    fn resolve_noop_without_target_item() {
        let map = ReExportMap::default();
        let mut dep = test_dep("my_crate", "render", None);
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "render");
    }

    // --- resolve_reexport: Cycle 2 — Crate not in map → dep unchanged ---

    #[test]
    fn resolve_noop_when_crate_not_in_map() {
        let map = ReExportMap::default();
        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "render");
    }

    // --- resolve_reexport: Cycle 3 — Own definition → dep unchanged ---

    #[test]
    fn resolve_noop_when_own_definition() {
        let mut module_info = ModuleExportInfo::default();
        module_info.definitions.insert("Widget".to_string());
        let mut crate_exports = HashMap::new();
        crate_exports.insert("render".to_string(), module_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "render");
    }

    // --- resolve_reexport: Cycle 4 — Explicit re-export → target_module updated ---

    #[test]
    fn resolve_explicit_reexport() {
        let mut m_info = ModuleExportInfo::default();
        m_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                module: "render::elements".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut n_info = ModuleExportInfo::default();
        n_info.definitions.insert("Widget".to_string());

        let mut crate_exports = HashMap::new();
        crate_exports.insert("render".to_string(), m_info);
        crate_exports.insert("render::elements".to_string(), n_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "render::elements");
    }

    // --- resolve_reexport: Cycle 5 — Transitive chain M → N → O ---

    #[test]
    fn resolve_transitive_chain() {
        let mut m_info = ModuleExportInfo::default();
        m_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                module: "middle".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut n_info = ModuleExportInfo::default();
        n_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                module: "origin".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut o_info = ModuleExportInfo::default();
        o_info.definitions.insert("Widget".to_string());

        let mut crate_exports = HashMap::new();
        crate_exports.insert("root".to_string(), m_info);
        crate_exports.insert("middle".to_string(), n_info);
        crate_exports.insert("origin".to_string(), o_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "root", Some("Widget"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "origin");
    }

    // --- resolve_reexport: Cycle 6 — Rename chain follows original_name ---

    #[test]
    fn resolve_rename_chain() {
        let mut m_info = ModuleExportInfo::default();
        m_info.explicit_reexports.insert(
            "Alias".to_string(),
            ReExportTarget {
                module: "origin".to_string(),
                original_name: "Original".to_string(),
            },
        );
        let mut o_info = ModuleExportInfo::default();
        o_info.definitions.insert("Original".to_string());

        let mut crate_exports = HashMap::new();
        crate_exports.insert("facade".to_string(), m_info);
        crate_exports.insert("origin".to_string(), o_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "facade", Some("Alias"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "origin");
    }

    // --- resolve_reexport: Cycle 7 — Glob resolution ---

    #[test]
    fn resolve_glob_reexport() {
        let mut m_info = ModuleExportInfo::default();
        m_info.glob_sources.push("elements".to_string());
        let mut n_info = ModuleExportInfo::default();
        n_info.definitions.insert("Widget".to_string());

        let mut crate_exports = HashMap::new();
        crate_exports.insert("render".to_string(), m_info);
        crate_exports.insert("elements".to_string(), n_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "render", Some("Widget"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "elements");
    }

    // --- resolve_reexport: Cycle 8 — Glob + transitive ---

    #[test]
    fn resolve_glob_then_explicit() {
        let mut m_info = ModuleExportInfo::default();
        m_info.glob_sources.push("middle".to_string());
        let mut n_info = ModuleExportInfo::default();
        n_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                module: "origin".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut o_info = ModuleExportInfo::default();
        o_info.definitions.insert("Widget".to_string());

        let mut crate_exports = HashMap::new();
        crate_exports.insert("facade".to_string(), m_info);
        crate_exports.insert("middle".to_string(), n_info);
        crate_exports.insert("origin".to_string(), o_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "facade", Some("Widget"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "origin");
    }

    // --- resolve_reexport: Cycle 9 — Cycle guard (no infinite loop) ---

    #[test]
    fn resolve_cycle_guard() {
        let mut a_info = ModuleExportInfo::default();
        a_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                module: "b".to_string(),
                original_name: "Widget".to_string(),
            },
        );
        let mut b_info = ModuleExportInfo::default();
        b_info.explicit_reexports.insert(
            "Widget".to_string(),
            ReExportTarget {
                module: "a".to_string(),
                original_name: "Widget".to_string(),
            },
        );

        let mut crate_exports = HashMap::new();
        crate_exports.insert("a".to_string(), a_info);
        crate_exports.insert("b".to_string(), b_info);
        let map: ReExportMap = [("my_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("my_crate", "a", Some("Widget"));
        resolve_reexport(&mut dep, &map);
        // Terminates without infinite loop; ends back at "a"
        assert_eq!(dep.target_module, "a");
    }

    // --- resolve_reexport: Cycle 10 — Cross-crate resolution ---

    #[test]
    fn resolve_cross_crate() {
        let mut root_info = ModuleExportInfo::default();
        root_info.explicit_reexports.insert(
            "Config".to_string(),
            ReExportTarget {
                module: "settings".to_string(),
                original_name: "Config".to_string(),
            },
        );
        let mut settings_info = ModuleExportInfo::default();
        settings_info.definitions.insert("Config".to_string());

        let mut crate_exports = HashMap::new();
        crate_exports.insert(String::new(), root_info);
        crate_exports.insert("settings".to_string(), settings_info);
        let map: ReExportMap = [("other_crate".to_string(), crate_exports)]
            .into_iter()
            .collect();

        let mut dep = test_dep("other_crate", "", Some("Config"));
        resolve_reexport(&mut dep, &map);
        assert_eq!(dep.target_module, "settings");
    }

    // --- Phase 1: Cycle 1 — ModuleExportInfo default ---

    #[test]
    fn is_empty_when_default() {
        let info = ModuleExportInfo::default();
        assert!(info.is_empty());
    }

    // --- Cycle 2: ReExportMap from_iter + deref ---

    #[test]
    fn from_iterator_and_deref() {
        let mut inner = HashMap::new();
        inner.insert("render".to_string(), ModuleExportInfo::default());
        let map: ReExportMap = [("my_crate".to_string(), inner)].into_iter().collect();
        assert!(map.contains_key("my_crate"));
        assert!(map["my_crate"].contains_key("render"));
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
