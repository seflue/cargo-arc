//! Dependency filtering logic for workspace analysis.
//!
//! This module handles:
//! - Classification of dependencies by kind (Normal/Dev/Build) and scope (Workspace/External)
//! - Feature string parsing
//! - Seed crate discovery based on feature configuration
//! - BFS traversal to collect reachable workspace crates

use cargo_metadata::{DependencyKind, Metadata, NodeDep};
use std::collections::{HashMap, HashSet, VecDeque};
use tracing::{debug, instrument};

use super::FeatureConfig;
use super::use_parser::is_workspace_member;

// --- Dependency filtering types ---

/// Dependency kind for filtering (internal use)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepKind {
    Normal,
    Dev,
    Build,
    Unknown,
}

/// Dependency scope for filtering (internal use)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepScope {
    Workspace,
    External,
}

/// Extracted dependency info for filtering and debugging
#[derive(Debug)]
pub(crate) struct DepInfo<'a> {
    pub(crate) name: &'a str,
    pub(crate) kind: DepKind,
    pub(crate) scope: DepScope,
}

impl<'a> DepInfo<'a> {
    /// Extract dependency info from a cargo metadata NodeDep
    pub(super) fn from_node_dep(dep: &'a NodeDep, workspace_members: &HashSet<&str>) -> Self {
        let name = dep.name.as_str();

        let kind = if dep
            .dep_kinds
            .iter()
            .any(|dk| matches!(dk.kind, DependencyKind::Normal))
        {
            DepKind::Normal
        } else if dep
            .dep_kinds
            .iter()
            .any(|dk| matches!(dk.kind, DependencyKind::Development))
        {
            DepKind::Dev
        } else if dep
            .dep_kinds
            .iter()
            .any(|dk| matches!(dk.kind, DependencyKind::Build))
        {
            DepKind::Build
        } else {
            DepKind::Unknown
        };

        // Normalize for comparison: cargo metadata uses underscores (core_utils),
        // but Cargo.toml names may have hyphens (core-utils)
        let scope = if is_workspace_member(name, workspace_members) {
            DepScope::Workspace
        } else {
            DepScope::External
        };

        Self { name, kind, scope }
    }

    /// Check if this dependency should be included in the workspace graph
    pub(super) fn is_included(&self) -> bool {
        matches!(self.kind, DepKind::Normal) && matches!(self.scope, DepScope::Workspace)
    }
}

/// Parses a feature string that may have a crate prefix.
/// Returns (crate_filter, feature_name) where crate_filter is Some if format is "crate/feature".
pub(super) fn parse_feature(feature: &str) -> (Option<&str>, &str) {
    match feature.split_once('/') {
        Some((crate_name, feat)) => (Some(crate_name), feat),
        None => (None, feature),
    }
}

/// Finds seed crates that define the requested features.
/// Returns all workspace members if no features specified or all_features is set.
#[instrument(skip_all, fields(features = ?feature_config.features, all_features = feature_config.all_features))]
pub(super) fn find_seed_crates(
    metadata: &Metadata,
    feature_config: &FeatureConfig,
    workspace_members: &HashSet<&str>,
) -> HashSet<String> {
    debug!(workspace_members = ?workspace_members);

    if feature_config.features.is_empty() || feature_config.all_features {
        debug!("returning ALL workspace members (no feature filter)");
        return workspace_members.iter().map(|s| s.to_string()).collect();
    }

    let seeds: HashSet<String> = metadata
        .packages
        .iter()
        .filter(|pkg| {
            let pkg_name = pkg.name.as_str();
            let is_workspace = workspace_members.contains(pkg_name);
            if !is_workspace {
                return false;
            }

            let pkg_features: Vec<&str> = pkg.features.keys().map(|s| s.as_str()).collect();
            debug!(pkg = pkg_name, features = ?pkg_features, "checking");

            let matches = feature_config.features.iter().any(|f| {
                let (crate_filter, feature_name) = parse_feature(f);
                let crate_matches = crate_filter.map(|c| c == pkg_name).unwrap_or(true);
                let feature_exists = pkg.features.contains_key(feature_name);

                debug!(
                    feature = f,
                    crate_filter = ?crate_filter,
                    crate_matches,
                    feature_exists,
                );

                crate_matches && feature_exists
            });

            debug!(pkg = pkg_name, matches);
            matches
        })
        .map(|pkg| pkg.name.to_string())
        .collect();

    debug!(seeds = ?seeds, "found");
    seeds
}

/// Collects all crates reachable from seeds via BFS through dependencies.
/// Only includes workspace members.
#[instrument(skip_all, fields(seed_count = seeds.len()))]
pub(super) fn collect_reachable_crates(
    seeds: HashSet<String>,
    resolved_deps: &HashMap<&str, Vec<String>>,
    workspace_members: &HashSet<&str>,
) -> HashSet<String> {
    debug!(seeds = ?seeds);
    for (pkg, deps) in resolved_deps {
        debug!(pkg, deps = ?deps, "resolved_dep");
    }

    let mut reachable: HashSet<String> = seeds.clone();
    let mut queue: VecDeque<String> = seeds.into_iter().collect();

    while let Some(crate_name) = queue.pop_front() {
        if let Some(deps) = resolved_deps.get(crate_name.as_str()) {
            for dep in deps {
                if workspace_members.contains(dep.as_str()) && !reachable.contains(dep) {
                    debug!(from = %crate_name, to = %dep, "BFS adding");
                    reachable.insert(dep.clone());
                    queue.push_back(dep.clone());
                }
            }
        }
    }

    debug!(reachable = ?reachable);
    reachable
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // parse_feature() tests
    // ========================================================================

    #[test]
    fn test_parse_feature_simple() {
        let (crate_filter, feature_name) = parse_feature("web");
        assert_eq!(crate_filter, None);
        assert_eq!(feature_name, "web");
    }

    #[test]
    fn test_parse_feature_with_crate_prefix() {
        let (crate_filter, feature_name) = parse_feature("app/web");
        assert_eq!(crate_filter, Some("app"));
        assert_eq!(feature_name, "web");
    }

    // ========================================================================
    // collect_reachable_crates() tests
    // ========================================================================

    #[test]
    fn test_collect_reachable_crates_bfs() {
        // A -> B -> C should traverse all three
        let seeds: HashSet<String> = ["A".to_string()].into_iter().collect();
        let mut resolved_deps: HashMap<&str, Vec<String>> = HashMap::new();
        resolved_deps.insert("A", vec!["B".to_string()]);
        resolved_deps.insert("B", vec!["C".to_string()]);
        let workspace: HashSet<&str> = ["A", "B", "C"].into_iter().collect();

        let reachable = collect_reachable_crates(seeds, &resolved_deps, &workspace);

        assert!(reachable.contains("A"));
        assert!(reachable.contains("B"));
        assert!(reachable.contains("C"));
        assert_eq!(reachable.len(), 3);
    }

    #[test]
    fn test_collect_reachable_stops_at_non_workspace() {
        // A -> B -> external (not in workspace) should stop at B
        let seeds: HashSet<String> = ["A".to_string()].into_iter().collect();
        let mut resolved_deps: HashMap<&str, Vec<String>> = HashMap::new();
        resolved_deps.insert("A", vec!["B".to_string()]);
        resolved_deps.insert("B", vec!["external".to_string()]);
        let workspace: HashSet<&str> = ["A", "B"].into_iter().collect();

        let reachable = collect_reachable_crates(seeds, &resolved_deps, &workspace);

        assert!(reachable.contains("A"));
        assert!(reachable.contains("B"));
        assert!(!reachable.contains("external"));
        assert_eq!(reachable.len(), 2);
    }

    #[test]
    fn test_collect_reachable_handles_cycles() {
        // A -> B -> A (cycle) should terminate
        let seeds: HashSet<String> = ["A".to_string()].into_iter().collect();
        let mut resolved_deps: HashMap<&str, Vec<String>> = HashMap::new();
        resolved_deps.insert("A", vec!["B".to_string()]);
        resolved_deps.insert("B", vec!["A".to_string()]);
        let workspace: HashSet<&str> = ["A", "B"].into_iter().collect();

        let reachable = collect_reachable_crates(seeds, &resolved_deps, &workspace);

        assert!(reachable.contains("A"));
        assert!(reachable.contains("B"));
        assert_eq!(reachable.len(), 2);
    }

    // --- DepInfo unit tests ---

    #[test]
    fn test_dep_info_normal_workspace_is_included() {
        let info = DepInfo {
            name: "foo",
            kind: DepKind::Normal,
            scope: DepScope::Workspace,
        };
        assert!(info.is_included(), "Normal + Workspace should be included");
    }

    #[test]
    fn test_dep_info_dev_workspace_is_excluded() {
        let info = DepInfo {
            name: "foo",
            kind: DepKind::Dev,
            scope: DepScope::Workspace,
        };
        assert!(!info.is_included(), "Dev + Workspace should be excluded");
    }

    #[test]
    fn test_dep_info_build_workspace_is_excluded() {
        let info = DepInfo {
            name: "foo",
            kind: DepKind::Build,
            scope: DepScope::Workspace,
        };
        assert!(!info.is_included(), "Build + Workspace should be excluded");
    }

    #[test]
    fn test_dep_info_normal_external_is_excluded() {
        let info = DepInfo {
            name: "serde",
            kind: DepKind::Normal,
            scope: DepScope::External,
        };
        assert!(
            !info.is_included(),
            "Normal + External should be excluded from workspace graph"
        );
    }

    #[test]
    fn test_dep_info_dev_external_is_excluded() {
        let info = DepInfo {
            name: "test-helper",
            kind: DepKind::Dev,
            scope: DepScope::External,
        };
        assert!(!info.is_included(), "Dev + External should be excluded");
    }
}
