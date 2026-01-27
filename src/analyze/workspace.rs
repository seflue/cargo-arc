//! Workspace analysis using cargo metadata.

use super::filtering::{DepInfo, collect_reachable_crates, find_seed_crates};
use super::hir::FeatureConfig;
use crate::model::CrateInfo;
use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use std::collections::HashSet;
use std::path::Path;
use tracing::debug;

// ============================================================================
// Workspace Analysis Helpers
// ============================================================================

/// Context built from cargo metadata for workspace analysis.
struct WorkspaceContext<'a> {
    pkg_id_to_name: std::collections::HashMap<&'a str, &'a str>,
    workspace_member_ids: HashSet<&'a str>,
    workspace_member_names: HashSet<&'a str>,
}

/// Runs cargo metadata with the given feature configuration.
fn run_cargo_metadata(
    manifest_path: &Path,
    feature_config: &FeatureConfig,
) -> Result<cargo_metadata::Metadata> {
    let mut cmd = MetadataCommand::new();
    cmd.manifest_path(manifest_path);

    if feature_config.all_features {
        cmd.features(cargo_metadata::CargoOpt::AllFeatures);
    } else if !feature_config.features.is_empty() {
        cmd.features(cargo_metadata::CargoOpt::SomeFeatures(
            feature_config.features.clone(),
        ));
    }
    if feature_config.no_default_features {
        cmd.features(cargo_metadata::CargoOpt::NoDefaultFeatures);
    }

    cmd.exec().context("Failed to run cargo metadata")
}

/// Builds workspace context from cargo metadata.
fn build_workspace_context(metadata: &cargo_metadata::Metadata) -> WorkspaceContext<'_> {
    let pkg_id_to_name = metadata
        .packages
        .iter()
        .map(|p| (p.id.repr.as_str(), p.name.as_str()))
        .collect();

    let workspace_member_ids = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let workspace_member_names = metadata
        .workspace_packages()
        .iter()
        .map(|p| p.name.as_str())
        .collect();

    WorkspaceContext {
        pkg_id_to_name,
        workspace_member_ids,
        workspace_member_names,
    }
}

/// Builds resolved dependencies map from cargo metadata resolve section.
fn build_resolved_deps<'a>(
    resolve: &'a cargo_metadata::Resolve,
    ctx: &WorkspaceContext<'a>,
) -> std::collections::HashMap<&'a str, Vec<String>> {
    let mut resolved_deps = std::collections::HashMap::new();

    debug!(workspace_members = ?ctx.workspace_member_names, "building resolved_deps");

    for node in &resolve.nodes {
        let node_id = node.id.repr.as_str();
        if !ctx.workspace_member_ids.contains(node_id) {
            continue;
        }

        let pkg_name = ctx.pkg_id_to_name.get(node_id).copied().unwrap_or("?");
        debug!(pkg = pkg_name, "processing deps");

        let deps: Vec<String> = node
            .deps
            .iter()
            .filter_map(|dep| {
                let info = DepInfo::from_node_dep(dep, &ctx.workspace_member_names);
                debug!(name = info.name, kind = ?info.kind, scope = ?info.scope);
                info.is_included().then(|| info.name.to_string())
            })
            .collect();

        if let Some(pkg_name) = ctx.pkg_id_to_name.get(node_id) {
            resolved_deps.insert(*pkg_name, deps);
        }
    }

    resolved_deps
}

/// Determines if a crate should be included based on feature config and reachability.
fn should_include_crate(
    pkg: &cargo_metadata::Package,
    reachable: &HashSet<String>,
    feature_config: &FeatureConfig,
) -> bool {
    let features_empty = feature_config.features.is_empty();
    let all_features = feature_config.all_features;
    let in_reachable = reachable.contains(pkg.name.as_str());
    let include = features_empty || all_features || in_reachable;

    debug!(
        crate_name = %pkg.name,
        features_empty,
        all_features,
        in_reachable,
        include
    );

    include
}

/// Builds a CrateInfo from a package and its resolved dependencies.
fn build_crate_info(
    pkg: &cargo_metadata::Package,
    resolved_deps: &std::collections::HashMap<&str, Vec<String>>,
) -> CrateInfo {
    let dependencies = resolved_deps
        .get(pkg.name.as_str())
        .cloned()
        .unwrap_or_default();

    CrateInfo {
        name: pkg.name.to_string(),
        path: pkg.manifest_path.parent().unwrap().into(),
        dependencies,
    }
}

/// Filters and builds CrateInfo list from workspace packages.
fn build_filtered_crates(
    metadata: &cargo_metadata::Metadata,
    resolved_deps: &std::collections::HashMap<&str, Vec<String>>,
    feature_config: &FeatureConfig,
    workspace_member_names: &HashSet<&str>,
) -> Vec<CrateInfo> {
    let seeds = find_seed_crates(metadata, feature_config, workspace_member_names);

    if seeds.is_empty() && !feature_config.features.is_empty() {
        eprintln!(
            "warning: No workspace crates define feature(s): {}",
            feature_config.features.join(", ")
        );
    }

    let reachable = collect_reachable_crates(seeds, resolved_deps, workspace_member_names);

    debug!(
        features_empty = feature_config.features.is_empty(),
        all_features = feature_config.all_features,
        "final crate filtering"
    );

    let crates: Vec<CrateInfo> = metadata
        .workspace_packages()
        .into_iter()
        .filter(|pkg| should_include_crate(pkg, &reachable, feature_config))
        .map(|pkg| build_crate_info(pkg, resolved_deps))
        .collect();

    debug!(crate_count = crates.len(), "final result");
    for c in &crates {
        debug!(crate_name = %c.name, deps = ?c.dependencies);
    }

    crates
}

/// Analyzes a workspace and returns all member crates.
/// `manifest_path` should point to a Cargo.toml.
/// `feature_config` controls which features are activated for dependency resolution.
pub fn analyze_workspace(
    manifest_path: &Path,
    feature_config: &FeatureConfig,
) -> Result<Vec<CrateInfo>> {
    let metadata = run_cargo_metadata(manifest_path, feature_config)?;

    let resolve = metadata
        .resolve
        .as_ref()
        .context("No resolve section in cargo metadata")?;

    let ctx = build_workspace_context(&metadata);
    let resolved_deps = build_resolved_deps(resolve, &ctx);
    let crates = build_filtered_crates(
        &metadata,
        &resolved_deps,
        feature_config,
        &ctx.workspace_member_names,
    );

    Ok(crates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Resolved dependency from cargo metadata's resolve section (test-only).
    #[derive(Debug, Clone)]
    struct ResolvedDependency {
        name: String,
        #[allow(dead_code)]
        pkg_id: String,
        dep_kinds: Vec<ResolvedDepKind>,
    }

    /// Dependency kind info from resolve section (test-only).
    #[derive(Debug, Clone)]
    struct ResolvedDepKind {
        kind: Option<String>,
        #[allow(dead_code)]
        target: Option<String>,
    }

    #[test]
    fn test_resolved_dependency_construction() {
        let dep = ResolvedDependency {
            name: "core".to_string(),
            pkg_id: "core 0.1.0 (path+file:///workspace/core)".to_string(),
            dep_kinds: vec![ResolvedDepKind {
                kind: None,
                target: None,
            }],
        };
        assert_eq!(dep.name, "core");
        assert_eq!(dep.dep_kinds.len(), 1);
        assert!(dep.dep_kinds[0].kind.is_none());
    }

    #[test]
    fn test_analyze_workspace_self() {
        // Test with cargo-arc itself as workspace
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates =
            analyze_workspace(&manifest, &FeatureConfig::default()).expect("should analyze");

        // cargo-arc should find itself
        assert!(!crates.is_empty());
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc");
        assert!(cargo_arc.is_some(), "should find cargo-arc");
    }

    #[test]
    fn test_crate_info_fields() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates =
            analyze_workspace(&manifest, &FeatureConfig::default()).expect("should analyze");

        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();
        assert!(cargo_arc.path.exists(), "path should exist");
        // dependencies is empty because cargo-arc has no workspace-internal deps
        // (only external: clap, petgraph, etc.)
    }

    // ========================================================================
    // Feature filtering tests (using feature_test_workspace fixture)
    // ========================================================================

    fn feature_test_manifest() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/feature_test_workspace/Cargo.toml")
    }

    #[test]
    fn test_feature_filtering_shows_all_crates() {
        // Without any features, all crates should be present
        let manifest = feature_test_manifest();
        let crates =
            analyze_workspace(&manifest, &FeatureConfig::default()).expect("should analyze");

        let names: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"core"), "should have core");
        assert!(names.contains(&"core-utils"), "should have core-utils");
        assert!(names.contains(&"server-utils"), "should have server-utils");
        assert!(names.contains(&"web-utils"), "should have web-utils");
    }

    #[test]
    fn test_feature_filtering_core_utils_depends_on_core() {
        // core-utils always depends on core (not optional)
        let manifest = feature_test_manifest();
        let crates =
            analyze_workspace(&manifest, &FeatureConfig::default()).expect("should analyze");

        let core_utils = crates.iter().find(|c| c.name == "core-utils").unwrap();
        assert!(
            core_utils.dependencies.contains(&"core".to_string()),
            "core-utils should depend on core, got: {:?}",
            core_utils.dependencies
        );
    }

    #[test]
    fn test_feature_filtering_server_without_feature() {
        // Without server feature, server-utils should NOT depend on core
        let manifest = feature_test_manifest();
        let crates =
            analyze_workspace(&manifest, &FeatureConfig::default()).expect("should analyze");

        let server_utils = crates.iter().find(|c| c.name == "server-utils").unwrap();
        assert!(
            !server_utils.dependencies.contains(&"core".to_string()),
            "server-utils should NOT depend on core without feature, got: {:?}",
            server_utils.dependencies
        );
    }

    #[test]
    fn test_feature_filtering_server_with_feature() {
        // With server feature, server-utils SHOULD depend on core
        let manifest = feature_test_manifest();
        let config = FeatureConfig {
            features: vec!["server-utils/server".to_string()],
            ..Default::default()
        };
        let crates = analyze_workspace(&manifest, &config).expect("should analyze");

        let server_utils = crates.iter().find(|c| c.name == "server-utils").unwrap();
        assert!(
            server_utils.dependencies.contains(&"core".to_string()),
            "server-utils SHOULD depend on core with server feature, got: {:?}",
            server_utils.dependencies
        );
    }

    #[test]
    fn test_feature_filtering_web_with_feature() {
        // With web feature, web-utils SHOULD depend on core
        let manifest = feature_test_manifest();
        let config = FeatureConfig {
            features: vec!["web-utils/web".to_string()],
            ..Default::default()
        };
        let crates = analyze_workspace(&manifest, &config).expect("should analyze");

        let web_utils = crates.iter().find(|c| c.name == "web-utils").unwrap();
        assert!(
            web_utils.dependencies.contains(&"core".to_string()),
            "web-utils SHOULD depend on core with web feature, got: {:?}",
            web_utils.dependencies
        );
    }

    #[test]
    fn test_node_id_matching_substring_names() {
        // Verify "core" and "core-utils" are correctly distinguished
        // This tests the Node-ID edge case mentioned in the plan
        let manifest = feature_test_manifest();
        let crates =
            analyze_workspace(&manifest, &FeatureConfig::default()).expect("should analyze");

        let core = crates.iter().find(|c| c.name == "core").unwrap();
        let core_utils = crates.iter().find(|c| c.name == "core-utils").unwrap();

        // core should have no workspace dependencies
        assert!(
            core.dependencies.is_empty(),
            "core should have no deps, got: {:?}",
            core.dependencies
        );

        // core-utils should depend on core and shared-lib (both normal workspace deps)
        assert!(
            core_utils.dependencies.contains(&"core".to_string()),
            "core-utils should depend on core, got: {:?}",
            core_utils.dependencies
        );
        assert!(
            core_utils.dependencies.contains(&"shared_lib".to_string()),
            "core-utils should depend on shared-lib (normalized: shared_lib), got: {:?}",
            core_utils.dependencies
        );
        assert_eq!(
            core_utils.dependencies.len(),
            2,
            "core-utils should have exactly 2 deps"
        );
    }

    // ========================================================================
    // Feature-based crate filtering tests (ACCEPTANCE CRITERIA)
    // ========================================================================

    #[test]
    fn test_feature_filtering_web_only_filters_crates() {
        // --features web: Only web-utils (defines "web") + its dependencies
        // web-utils has: core (optional, activated by "web"), testlib (normal dep)
        // Should NOT include: server-utils, core-utils, shared-lib, build-helper
        let manifest = feature_test_manifest();
        let config = FeatureConfig {
            features: vec!["web".to_string()],
            ..Default::default()
        };
        let crates = analyze_workspace(&manifest, &config).expect("should analyze");

        let names: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"web-utils"),
            "should have web-utils, got: {:?}",
            names
        );
        assert!(
            names.contains(&"core"),
            "should have core (dependency), got: {:?}",
            names
        );
        assert!(
            names.contains(&"testlib"),
            "should have testlib (normal dep of web-utils), got: {:?}",
            names
        );
        assert!(
            !names.contains(&"server-utils"),
            "should NOT have server-utils, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"core-utils"),
            "should NOT have core-utils, got: {:?}",
            names
        );
        assert_eq!(names.len(), 3, "expected 3 crates, got: {:?}", names);
    }

    #[test]
    fn test_feature_filtering_server_only_filters_crates() {
        // --features server: Only server-utils (defines "server") + core (dependency)
        let manifest = feature_test_manifest();
        let config = FeatureConfig {
            features: vec!["server".to_string()],
            ..Default::default()
        };
        let crates = analyze_workspace(&manifest, &config).expect("should analyze");

        let names: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"server-utils"),
            "should have server-utils, got: {:?}",
            names
        );
        assert!(
            names.contains(&"core"),
            "should have core (dependency), got: {:?}",
            names
        );
        assert!(
            !names.contains(&"web-utils"),
            "should NOT have web-utils, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"core-utils"),
            "should NOT have core-utils, got: {:?}",
            names
        );
        assert_eq!(names.len(), 2, "expected 2 crates, got: {:?}", names);
    }

    // ========================================================================
    // Edge case tests
    // ========================================================================

    #[test]
    fn test_feature_filtering_unknown_feature_returns_error() {
        // Unknown feature causes cargo metadata to fail (cargo validates features)
        let manifest = feature_test_manifest();
        let config = FeatureConfig {
            features: vec!["nonexistent".to_string()],
            ..Default::default()
        };
        let result = analyze_workspace(&manifest, &config);

        assert!(
            result.is_err(),
            "unknown feature should cause cargo metadata to fail"
        );
    }

    #[test]
    fn test_feature_filtering_all_features_shows_all() {
        // --all-features should show all workspace crates
        let manifest = feature_test_manifest();
        let config = FeatureConfig {
            all_features: true,
            ..Default::default()
        };
        let crates = analyze_workspace(&manifest, &config).expect("should analyze");

        let names: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"core"), "should have core");
        assert!(names.contains(&"core-utils"), "should have core-utils");
        assert!(names.contains(&"server-utils"), "should have server-utils");
        assert!(names.contains(&"web-utils"), "should have web-utils");
        assert!(names.contains(&"testlib"), "should have testlib");
        assert!(names.contains(&"shared-lib"), "should have shared-lib");
        assert!(names.contains(&"build-helper"), "should have build-helper");
        assert_eq!(names.len(), 7, "expected all 7 crates, got: {:?}", names);
    }
}
