//! External crate dependency analysis using cargo metadata.

use crate::model::{
    EdgeContext, ExternalDep, ExternalPackageInfo, ExternalsResult, TestKind, WorkspaceExternalDep,
};
use cargo_metadata::{DependencyKind, Metadata};
use std::collections::{HashMap, HashSet, VecDeque};

fn is_relevant_dep(dep: &cargo_metadata::NodeDep) -> bool {
    dep.dep_kinds.iter().any(|dk| {
        matches!(
            dk.kind,
            DependencyKind::Normal | DependencyKind::Development
        )
    })
}

fn build_external_crate_info(pkg: &cargo_metadata::Package) -> ExternalPackageInfo {
    ExternalPackageInfo {
        name: pkg.name.to_string(),
        version: pkg.version.to_string(),
        package_id: pkg.id.repr.clone(),
        entry_files: super::workspace::entry_files(pkg),
        manifest: Some(pkg.manifest_path.clone().into()),
    }
}

/// Production when any kind is normal, a test edge when only dev kinds
/// remain, `None` for a build-only dependency.
fn edge_context(dep: &cargo_metadata::NodeDep) -> Option<EdgeContext> {
    let kinds = || dep.dep_kinds.iter().map(|dk| dk.kind);
    if kinds().any(|kind| kind == DependencyKind::Normal) {
        Some(EdgeContext::production())
    } else if kinds().any(|kind| kind == DependencyKind::Development) {
        Some(EdgeContext::test(TestKind::Unit))
    } else {
        None
    }
}

/// `workspace_crate_name` -> (`code_name` -> `package_id`).
/// Per-workspace-package map because different workspace packages can depend on
/// different versions of the same external package.
pub(crate) type CrateNameMap = HashMap<String, HashMap<String, String>>;

struct ReachableExternals {
    seen: HashMap<String, ExternalPackageInfo>,
    crate_name_map: CrateNameMap,
}

/// BFS from workspace direct dependencies to collect all transitively reachable
/// external nodes. Guarantees completeness regardless of resolve.nodes order.
fn collect_reachable_externals(
    resolve: &cargo_metadata::Resolve,
    workspace_member_ids: &HashSet<&str>,
    pkg_by_id: &HashMap<&str, &cargo_metadata::Package>,
    transitive: bool,
) -> ReachableExternals {
    let deps_by_id: HashMap<&str, &[cargo_metadata::NodeDep]> = resolve
        .nodes
        .iter()
        .map(|n| (n.id.repr.as_str(), n.deps.as_slice()))
        .collect();

    let mut seen: HashMap<String, ExternalPackageInfo> = HashMap::new();
    let mut crate_name_map = CrateNameMap::new();
    let mut bfs_queue: VecDeque<String> = VecDeque::new();

    // Seed: workspace nodes' direct external dependencies
    for node in &resolve.nodes {
        let node_id = node.id.repr.as_str();
        if !workspace_member_ids.contains(node_id) {
            continue;
        }

        let ws_name = pkg_by_id.get(node_id).map_or("?", |p| p.name.as_str());
        let normalized_ws_name = crate::model::normalize_crate_name(ws_name);

        for dep in &node.deps {
            let dep_id = dep.pkg.repr.as_str();
            let Some(dep_pkg) = pkg_by_id.get(dep_id) else {
                continue;
            };
            if dep_pkg.source.is_none() || !is_relevant_dep(dep) {
                continue;
            }

            if !seen.contains_key(dep_id) {
                seen.insert(dep_id.to_string(), build_external_crate_info(dep_pkg));
                if transitive {
                    bfs_queue.push_back(dep_id.to_string());
                }
            }

            // Build crate_name_map: code-side name -> package_id
            // dep.name is the library target name (includes renames and - -> _ mapping)
            crate_name_map
                .entry(normalized_ws_name.clone())
                .or_default()
                .insert(dep.name.clone(), dep_id.to_string());
        }
    }

    // BFS: follow external -> external edges to collect all transitively reachable nodes
    while let Some(ext_id) = bfs_queue.pop_front() {
        let Some(deps) = deps_by_id.get(ext_id.as_str()) else {
            continue;
        };
        for dep in *deps {
            let dep_id = dep.pkg.repr.as_str();
            let Some(dep_pkg) = pkg_by_id.get(dep_id) else {
                continue;
            };
            if dep_pkg.source.is_none() || !is_relevant_dep(dep) {
                continue;
            }
            if !seen.contains_key(dep_id) {
                seen.insert(dep_id.to_string(), build_external_crate_info(dep_pkg));
                bfs_queue.push_back(dep_id.to_string());
            }
        }
    }

    ReachableExternals {
        seen,
        crate_name_map,
    }
}

/// Analyze external crate dependencies from cargo metadata, and map each
/// workspace package's code-side names of them to their package ids.
///
/// When `transitive` is false, only direct workspace→external edges are collected.
/// When true, also collects transitive extern→extern edges via full BFS.
pub(crate) fn analyze_externals(
    metadata: &Metadata,
    transitive: bool,
) -> (ExternalsResult, CrateNameMap) {
    let Some(resolve) = metadata.resolve.as_ref() else {
        let empty = ExternalsResult {
            packages: Vec::new(),
            external_deps: Vec::new(),
            workspace_deps: Vec::new(),
        };
        return (empty, CrateNameMap::new());
    };

    let workspace_member_ids: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let pkg_by_id: HashMap<&str, &cargo_metadata::Package> = metadata
        .packages
        .iter()
        .map(|p| (p.id.repr.as_str(), p))
        .collect();

    let reachable =
        collect_reachable_externals(resolve, &workspace_member_ids, &pkg_by_id, transitive);

    // All reachable nodes are now collected, so edge creation is independent
    // of resolve.nodes iteration order.
    let mut external_deps: Vec<ExternalDep> = Vec::new();
    let mut workspace_deps: Vec<WorkspaceExternalDep> = Vec::new();

    for node in &resolve.nodes {
        let node_id = node.id.repr.as_str();
        let is_workspace = workspace_member_ids.contains(node_id);

        for dep in &node.deps {
            let dep_id = dep.pkg.repr.as_str();
            if !reachable.seen.contains_key(dep_id) {
                continue;
            }

            let Some(context) = edge_context(dep) else {
                continue;
            };

            if is_workspace {
                let ws_name = pkg_by_id.get(node_id).map_or("?", |p| p.name.as_str());

                workspace_deps.push(WorkspaceExternalDep {
                    workspace_package: crate::model::normalize_crate_name(ws_name),
                    external_pkg_id: dep_id.to_string(),
                    context,
                });
            } else if transitive && reachable.seen.contains_key(node_id) {
                external_deps.push(ExternalDep {
                    from_pkg_id: node_id.to_string(),
                    to_pkg_id: dep_id.to_string(),
                    context,
                });
            }
        }
    }

    let result = ExternalsResult {
        packages: reachable.seen.into_values().collect(),
        external_deps,
        workspace_deps,
    };
    (result, reachable.crate_name_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EdgeContext, EntryKind, TestKind};
    use cargo_metadata::MetadataCommand;
    use std::path::Path;

    fn own_metadata() -> Metadata {
        MetadataCommand::new()
            .manifest_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .exec()
            .expect("cargo metadata should succeed")
    }

    #[test]
    fn test_analyze_externals_self() {
        let metadata = own_metadata();
        let (result, crate_name_map) = analyze_externals(&metadata, false);

        // cargo-arc has external deps like petgraph, syn, clap
        assert!(!result.packages.is_empty(), "should find external crates");
        assert!(
            !result.workspace_deps.is_empty(),
            "should find workspace->external deps"
        );

        // cargo_metadata itself should be in the crate_name_map
        let ws_name = crate::model::normalize_crate_name("cargo-arc");
        assert!(
            crate_name_map.contains_key(&ws_name),
            "crate_name_map should contain cargo_arc, got keys: {:?}",
            crate_name_map.keys().collect::<Vec<_>>()
        );
        let inner = &crate_name_map[&ws_name];
        assert!(
            inner.contains_key("cargo_metadata"),
            "inner map should contain cargo_metadata, got keys: {:?}",
            inner.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_analyze_externals_known_crates() {
        let metadata = own_metadata();
        let (result, _) = analyze_externals(&metadata, false);

        let crate_names: Vec<&str> = result.packages.iter().map(|c| c.name.as_str()).collect();
        assert!(
            crate_names.contains(&"petgraph"),
            "should find petgraph, got: {crate_names:?}"
        );
        assert!(
            crate_names.contains(&"syn"),
            "should find syn, got: {crate_names:?}"
        );
        assert!(
            crate_names.contains(&"clap"),
            "should find clap, got: {crate_names:?}"
        );
    }

    #[test]
    fn test_external_crate_keeps_its_targets() {
        let metadata = own_metadata();
        let (result, _) = analyze_externals(&metadata, false);

        let petgraph = result
            .packages
            .iter()
            .find(|c| c.name == "petgraph")
            .expect("petgraph is a dependency");
        let [lib_root] = petgraph.entry_files.as_slice() else {
            panic!(
                "petgraph has one entry file, got {:?}",
                petgraph.entry_files
            );
        };
        assert_eq!(lib_root.kind, EntryKind::Lib);
        assert!(
            lib_root.file.is_absolute(),
            "got {}",
            lib_root.file.display()
        );
        assert!(
            lib_root.file.ends_with("src/lib.rs"),
            "got {}",
            lib_root.file.display()
        );
        let manifest = petgraph.manifest.as_ref().expect("a manifest");
        assert!(manifest.is_absolute(), "got {}", manifest.display());
        assert!(
            manifest.ends_with("Cargo.toml"),
            "got {}",
            manifest.display()
        );
    }

    #[test]
    fn test_analyze_externals_no_workspace_crates_in_externals() {
        let metadata = own_metadata();
        let (result, _) = analyze_externals(&metadata, false);

        // cargo-arc itself should NOT appear as an external crate
        let external_names: Vec<&str> = result.packages.iter().map(|c| c.name.as_str()).collect();
        assert!(
            !external_names.contains(&"cargo-arc"),
            "workspace crate should not appear in externals"
        );
    }

    #[test]
    fn dev_only_externals_arrive_as_test_edges() {
        let metadata = own_metadata();
        let (result, _) = analyze_externals(&metadata, false);
        let context_to = |name: &str| {
            let id = &result
                .packages
                .iter()
                .find(|p| p.name == name)
                .expect("a dependency of cargo-arc")
                .package_id;
            result
                .workspace_deps
                .iter()
                .find(|dep| &dep.external_pkg_id == id)
                .expect("a direct dependency")
                .context
                .clone()
        };

        assert_eq!(context_to("petgraph"), EdgeContext::production());
        assert_eq!(context_to("tempfile"), EdgeContext::test(TestKind::Unit));
    }

    #[test]
    fn test_transitive_no_orphan_nodes() {
        let metadata = own_metadata();
        let (result, _) = analyze_externals(&metadata, true);

        assert!(
            !result.external_deps.is_empty(),
            "transitive mode should produce external->external edges"
        );

        // Collect all package_ids that appear in at least one edge
        let mut connected: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for dep in &result.workspace_deps {
            connected.insert(dep.external_pkg_id.as_str());
        }
        for dep in &result.external_deps {
            connected.insert(dep.from_pkg_id.as_str());
            connected.insert(dep.to_pkg_id.as_str());
        }

        // Every collected crate must appear in at least one edge
        let orphans: Vec<&str> = result
            .packages
            .iter()
            .map(|c| c.package_id.as_str())
            .filter(|id| !connected.contains(id))
            .collect();

        assert!(
            orphans.is_empty(),
            "found {} orphan nodes (no incoming or outgoing edges): {:?}",
            orphans.len(),
            orphans
        );
    }
}
