//! External crate dependency analysis using cargo metadata.

use crate::model::{ExternalCrateInfo, ExternalDep, ExternalsResult, WorkspaceExternalDep};
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

fn build_external_crate_info(pkg: &cargo_metadata::Package) -> ExternalCrateInfo {
    ExternalCrateInfo {
        name: pkg.name.to_string(),
        version: pkg.version.to_string(),
        package_id: pkg.id.repr.clone(),
        target_roots: super::workspace::target_roots(pkg),
    }
}

fn collect_dep_kinds(dep: &cargo_metadata::NodeDep) -> Vec<DependencyKind> {
    dep.dep_kinds
        .iter()
        .filter(|dk| {
            matches!(
                dk.kind,
                DependencyKind::Normal | DependencyKind::Development
            )
        })
        .map(|dk| dk.kind)
        .collect()
}

struct ReachableExternals {
    seen: HashMap<String, ExternalCrateInfo>,
    crate_name_map: HashMap<String, HashMap<String, String>>,
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

    let mut seen: HashMap<String, ExternalCrateInfo> = HashMap::new();
    let mut crate_name_map: HashMap<String, HashMap<String, String>> = HashMap::new();
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

/// Analyze external crate dependencies from cargo metadata.
///
/// When `transitive` is false, only direct workspace→external edges are collected.
/// When true, also collects transitive extern→extern edges via full BFS.
pub(crate) fn analyze_externals(metadata: &Metadata, transitive: bool) -> ExternalsResult {
    let Some(resolve) = metadata.resolve.as_ref() else {
        return ExternalsResult {
            crates: Vec::new(),
            external_deps: Vec::new(),
            workspace_deps: Vec::new(),
            crate_name_map: HashMap::new(),
        };
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

            let dep_kinds = collect_dep_kinds(dep);
            if dep_kinds.is_empty() {
                continue;
            }

            if is_workspace {
                let ws_name = pkg_by_id.get(node_id).map_or("?", |p| p.name.as_str());

                workspace_deps.push(WorkspaceExternalDep {
                    workspace_crate: crate::model::normalize_crate_name(ws_name),
                    external_pkg_id: dep_id.to_string(),
                    dep_kinds,
                });
            } else if transitive && reachable.seen.contains_key(node_id) {
                external_deps.push(ExternalDep {
                    from_pkg_id: node_id.to_string(),
                    to_pkg_id: dep_id.to_string(),
                    dep_kinds,
                });
            }
        }
    }

    ExternalsResult {
        crates: reachable.seen.into_values().collect(),
        external_deps,
        workspace_deps,
        crate_name_map: reachable.crate_name_map,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TargetRoots;
    use cargo_metadata::MetadataCommand;
    use std::path::Path;

    fn own_metadata() -> Metadata {
        MetadataCommand::new()
            .manifest_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .exec()
            .expect("cargo metadata should succeed")
    }

    #[test]
    fn test_externals_result_construction() {
        let result = ExternalsResult {
            crates: vec![ExternalCrateInfo {
                name: "serde".to_string(),
                version: "1.0.0".to_string(),
                package_id: "serde 1.0.0 (registry+...)".to_string(),
                target_roots: TargetRoots::default(),
            }],
            external_deps: vec![ExternalDep {
                from_pkg_id: "serde 1.0.0".to_string(),
                to_pkg_id: "serde_derive 1.0.0".to_string(),
                dep_kinds: vec![DependencyKind::Normal],
            }],
            workspace_deps: vec![WorkspaceExternalDep {
                workspace_crate: "my_crate".to_string(),
                external_pkg_id: "serde 1.0.0".to_string(),
                dep_kinds: vec![DependencyKind::Normal],
            }],
            crate_name_map: {
                let mut outer = HashMap::new();
                let mut inner = HashMap::new();
                inner.insert("serde".to_string(), "serde 1.0.0".to_string());
                outer.insert("my_crate".to_string(), inner);
                outer
            },
        };

        assert_eq!(result.crates.len(), 1);
        assert_eq!(result.crates[0].name, "serde");
        assert_eq!(result.external_deps.len(), 1);
        assert_eq!(result.workspace_deps.len(), 1);
        assert_eq!(result.crate_name_map["my_crate"]["serde"], "serde 1.0.0");
    }

    #[test]
    fn test_analyze_externals_self() {
        let metadata = own_metadata();
        let result = analyze_externals(&metadata, false);

        // cargo-arc has external deps like petgraph, syn, clap
        assert!(!result.crates.is_empty(), "should find external crates");
        assert!(
            !result.workspace_deps.is_empty(),
            "should find workspace->external deps"
        );

        // cargo_metadata itself should be in the crate_name_map
        let ws_name = crate::model::normalize_crate_name("cargo-arc");
        assert!(
            result.crate_name_map.contains_key(&ws_name),
            "crate_name_map should contain cargo_arc, got keys: {:?}",
            result.crate_name_map.keys().collect::<Vec<_>>()
        );
        let inner = &result.crate_name_map[&ws_name];
        assert!(
            inner.contains_key("cargo_metadata"),
            "inner map should contain cargo_metadata, got keys: {:?}",
            inner.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_analyze_externals_known_crates() {
        let metadata = own_metadata();
        let result = analyze_externals(&metadata, false);

        let crate_names: Vec<&str> = result.crates.iter().map(|c| c.name.as_str()).collect();
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
        let result = analyze_externals(&metadata, false);

        let petgraph = result
            .crates
            .iter()
            .find(|c| c.name == "petgraph")
            .expect("petgraph is a dependency");
        let lib_root = petgraph
            .target_roots
            .lib_root
            .as_ref()
            .expect("petgraph has a lib target");
        assert!(lib_root.is_absolute(), "got {}", lib_root.display());
        assert!(
            lib_root.ends_with("src/lib.rs"),
            "got {}",
            lib_root.display()
        );
        assert!(petgraph.target_roots.bin_roots.is_empty());
    }

    #[test]
    fn test_analyze_externals_no_workspace_crates_in_externals() {
        let metadata = own_metadata();
        let result = analyze_externals(&metadata, false);

        // cargo-arc itself should NOT appear as an external crate
        let external_names: Vec<&str> = result.crates.iter().map(|c| c.name.as_str()).collect();
        assert!(
            !external_names.contains(&"cargo-arc"),
            "workspace crate should not appear in externals"
        );
    }

    #[test]
    fn test_analyze_externals_filters_build_only_deps() {
        let metadata = own_metadata();
        let result = analyze_externals(&metadata, false);

        // All workspace_deps should have Normal or Development kinds, not Build-only
        for dep in &result.workspace_deps {
            assert!(
                dep.dep_kinds
                    .iter()
                    .any(|k| matches!(k, DependencyKind::Normal | DependencyKind::Development)),
                "workspace dep to {} should have Normal or Dev kind, got: {:?}",
                dep.external_pkg_id,
                dep.dep_kinds
            );
        }
    }

    #[test]
    fn test_transitive_no_orphan_nodes() {
        let metadata = own_metadata();
        let result = analyze_externals(&metadata, true);

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
            .crates
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
