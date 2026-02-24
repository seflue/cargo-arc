//! External crate dependency analysis using cargo metadata.

// Some fields (dep_kinds, crate_name_map) are consumed by later phases
// (layout, use-parser integration) and appear unused until then.
#![allow(dead_code)]

use cargo_metadata::{DependencyKind, Metadata};
use std::collections::HashMap;

/// Metadata for a single external crate (one entry per version).
#[derive(Debug, Clone)]
pub(crate) struct ExternalCrateInfo {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) package_id: String,
}

/// Dependency edge between two external crates.
#[derive(Debug, Clone)]
pub(crate) struct ExternalDep {
    pub(crate) from_pkg_id: String,
    pub(crate) to_pkg_id: String,
    pub(crate) dep_kinds: Vec<DependencyKind>,
}

/// Dependency edge from a workspace crate to an external crate.
#[derive(Debug, Clone)]
pub(crate) struct WorkspaceExternalDep {
    pub(crate) workspace_crate: String,
    pub(crate) external_pkg_id: String,
    pub(crate) dep_kinds: Vec<DependencyKind>,
}

/// Result of external dependency analysis from cargo metadata.
#[derive(Debug)]
pub(crate) struct ExternalsResult {
    pub(crate) crates: Vec<ExternalCrateInfo>,
    pub(crate) external_deps: Vec<ExternalDep>,
    pub(crate) workspace_deps: Vec<WorkspaceExternalDep>,
    /// `workspace_crate_name` -> (`code_name` -> `package_id`).
    /// Per-workspace-crate map because different workspace crates can depend on
    /// different versions of the same external crate.
    pub(crate) crate_name_map: HashMap<String, HashMap<String, String>>,
}

/// Analyze external crate dependencies from cargo metadata.
///
/// BFS over `resolve.nodes[].deps` starting from `workspace_members`.
/// Collects external crates (`pkg.source.is_some()`), their inter-dependencies,
/// and the name mapping per workspace crate for use-parser resolution.
pub(crate) fn analyze_externals(metadata: &Metadata) -> ExternalsResult {
    let Some(resolve) = metadata.resolve.as_ref() else {
        return ExternalsResult {
            crates: Vec::new(),
            external_deps: Vec::new(),
            workspace_deps: Vec::new(),
            crate_name_map: HashMap::new(),
        };
    };

    let workspace_member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let pkg_by_id: HashMap<&str, &cargo_metadata::Package> = metadata
        .packages
        .iter()
        .map(|p| (p.id.repr.as_str(), p))
        .collect();

    let mut seen_externals: HashMap<String, ExternalCrateInfo> = HashMap::new();
    let external_deps: Vec<ExternalDep> = Vec::new();
    let mut workspace_deps: Vec<WorkspaceExternalDep> = Vec::new();
    let mut crate_name_map: HashMap<String, HashMap<String, String>> = HashMap::new();

    for node in &resolve.nodes {
        let node_id = node.id.repr.as_str();
        let is_workspace = workspace_member_ids.contains(node_id);
        let node_pkg = pkg_by_id.get(node_id);

        for dep in &node.deps {
            let dep_id = dep.pkg.repr.as_str();
            let Some(dep_pkg) = pkg_by_id.get(dep_id) else {
                continue;
            };

            // Only include production and dev dependencies (skip build)
            let dep_kinds: Vec<DependencyKind> = dep
                .dep_kinds
                .iter()
                .filter(|dk| {
                    matches!(
                        dk.kind,
                        DependencyKind::Normal | DependencyKind::Development
                    )
                })
                .map(|dk| dk.kind)
                .collect();

            if dep_kinds.is_empty() {
                continue;
            }

            // External crate: has a source (registry, git, etc.)
            let dep_is_external = dep_pkg.source.is_some();
            if !dep_is_external {
                continue;
            }

            // Only direct workspace dependencies — skip transitive extern→extern
            if !is_workspace {
                continue;
            }

            // Record external crate info (deduplicated by package_id)
            seen_externals
                .entry(dep_id.to_string())
                .or_insert_with(|| ExternalCrateInfo {
                    name: dep_pkg.name.to_string(),
                    version: dep_pkg.version.to_string(),
                    package_id: dep_id.to_string(),
                });

            // Workspace -> external edge
            let ws_name = node_pkg.map_or("?", |p| p.name.as_str());
            let normalized_ws_name = crate::model::normalize_crate_name(ws_name);

            workspace_deps.push(WorkspaceExternalDep {
                workspace_crate: normalized_ws_name.clone(),
                external_pkg_id: dep_id.to_string(),
                dep_kinds: dep_kinds.clone(),
            });

            // Build crate_name_map: code-side name -> package_id
            // dep.name is the library target name (includes renames and - -> _ mapping)
            crate_name_map
                .entry(normalized_ws_name)
                .or_default()
                .insert(dep.name.clone(), dep_id.to_string());
        }
    }

    ExternalsResult {
        crates: seen_externals.into_values().collect(),
        external_deps,
        workspace_deps,
        crate_name_map,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let result = analyze_externals(&metadata);

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
        let result = analyze_externals(&metadata);

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
    fn test_analyze_externals_no_workspace_crates_in_externals() {
        let metadata = own_metadata();
        let result = analyze_externals(&metadata);

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
        let result = analyze_externals(&metadata);

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
}
