//! Workspace & Module Analysis

mod use_parser;
use use_parser::{is_workspace_member, normalize_crate_name, parse_workspace_dependencies};

use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use ra_ap_cfg::{CfgAtom, CfgDiff};
use ra_ap_hir as hir;
use ra_ap_ide as ide;
use ra_ap_load_cargo as load_cargo;
use ra_ap_paths as paths;
use ra_ap_project_model as project_model;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::model::{CrateInfo, DependencyRef, ModuleInfo, ModuleTree};
use tracing::{debug, instrument};

#[derive(Debug, Clone, Default)]
pub struct FeatureConfig {
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub cfg_flags: Vec<String>,
    pub debug: bool,
}

/// Resolved dependency from cargo metadata's resolve section.
/// Contains the actual dependency graph after feature resolution.
#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    pub name: String,
    pub pkg_id: String,
    pub dep_kinds: Vec<ResolvedDepKind>,
}

/// Dependency kind info from resolve section.
#[derive(Debug, Clone)]
pub struct ResolvedDepKind {
    pub kind: Option<String>,
    pub target: Option<String>,
}

// --- Dependency filtering types ---

/// Dependency kind for filtering (internal use)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepKind {
    Normal,
    Dev,
    Build,
    Unknown,
}

/// Dependency scope for filtering (internal use)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepScope {
    Workspace,
    External,
}

/// Extracted dependency info for filtering and debugging
#[derive(Debug)]
struct DepInfo<'a> {
    name: &'a str,
    kind: DepKind,
    scope: DepScope,
}

impl<'a> DepInfo<'a> {
    /// Extract dependency info from a cargo metadata NodeDep
    fn from_node_dep(dep: &'a cargo_metadata::NodeDep, workspace_members: &HashSet<&str>) -> Self {
        let name = dep.name.as_str();

        let kind = if dep
            .dep_kinds
            .iter()
            .any(|dk| matches!(dk.kind, cargo_metadata::DependencyKind::Normal))
        {
            DepKind::Normal
        } else if dep
            .dep_kinds
            .iter()
            .any(|dk| matches!(dk.kind, cargo_metadata::DependencyKind::Development))
        {
            DepKind::Dev
        } else if dep
            .dep_kinds
            .iter()
            .any(|dk| matches!(dk.kind, cargo_metadata::DependencyKind::Build))
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
    fn is_included(&self) -> bool {
        matches!(self.kind, DepKind::Normal) && matches!(self.scope, DepScope::Workspace)
    }
}

/// Creates a CargoConfig with feature and cfg overrides.
/// By default, cfg(test) is disabled.
pub fn cargo_config_with_features(config: &FeatureConfig) -> project_model::CargoConfig {
    let features = if config.all_features {
        project_model::CargoFeatures::All
    } else if config.features.is_empty() && !config.no_default_features {
        project_model::CargoFeatures::default()
    } else {
        project_model::CargoFeatures::Selected {
            features: config.features.clone(),
            no_default_features: config.no_default_features,
        }
    };

    // Build enable list: features as KeyValue atoms, optionally test flag
    let mut enable_cfgs: Vec<CfgAtom> = config
        .features
        .iter()
        .map(|f| CfgAtom::KeyValue {
            key: hir::Symbol::intern("feature"),
            value: hir::Symbol::intern(f),
        })
        .collect();

    let include_test = config.cfg_flags.contains(&"test".to_string());
    if include_test {
        enable_cfgs.push(CfgAtom::Flag(hir::Symbol::intern("test")));
    }

    // Build disable list: test flag unless explicitly enabled
    let disable_cfgs = if include_test {
        Vec::new()
    } else {
        vec![CfgAtom::Flag(hir::Symbol::intern("test"))]
    };

    let cfg_overrides = project_model::CfgOverrides {
        global: CfgDiff::new(enable_cfgs, disable_cfgs),
        selective: Default::default(),
    };

    project_model::CargoConfig {
        features,
        cfg_overrides,
        sysroot: Some(project_model::RustLibSource::Discover),
        ..Default::default()
    }
}

/// Parses a feature string that may have a crate prefix.
/// Returns (crate_filter, feature_name) where crate_filter is Some if format is "crate/feature".
fn parse_feature(feature: &str) -> (Option<&str>, &str) {
    match feature.split_once('/') {
        Some((crate_name, feat)) => (Some(crate_name), feat),
        None => (None, feature),
    }
}

/// Finds seed crates that define the requested features.
/// Returns all workspace members if no features specified or all_features is set.
#[instrument(skip_all, fields(features = ?feature_config.features, all_features = feature_config.all_features))]
fn find_seed_crates(
    metadata: &cargo_metadata::Metadata,
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
fn collect_reachable_crates(
    seeds: HashSet<String>,
    resolved_deps: &std::collections::HashMap<&str, Vec<String>>,
    workspace_members: &HashSet<&str>,
) -> HashSet<String> {
    use std::collections::VecDeque;

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

// ============================================================================
// Module Hierarchy Analysis (via ra_ap_hir)
// ============================================================================

/// Analyzes the module hierarchy of a crate using rust-analyzer's HIR.
/// The `host` and `vfs` should be obtained from `load_workspace_hir()`.
/// `workspace_crates` should contain all workspace crate names for inter-crate dependency detection.
pub fn analyze_modules(
    crate_info: &CrateInfo,
    host: &ide::AnalysisHost,
    vfs: &ra_ap_vfs::Vfs,
    workspace_crates: &HashSet<String>,
) -> Result<ModuleTree> {
    // Find crate in already-loaded workspace
    let krate = find_crate_in_workspace(crate_info, host, vfs)?;
    let db = host.raw_database();

    // Walk module tree starting from crate root
    let root_module = krate.root_module(db);
    let crate_name = &crate_info.name;
    // Use actual crate name (normalized) as root path for inter-crate dependency resolution
    let normalized_crate_name = normalize_crate_name(crate_name);
    let root = walk_module(
        root_module,
        db,
        vfs,
        &normalized_crate_name,
        &crate_info.path,
        crate_name,
        workspace_crates,
    );

    Ok(ModuleTree { root })
}

fn walk_module(
    module: hir::Module,
    db: &ide::RootDatabase,
    vfs: &ra_ap_vfs::Vfs,
    parent_path: &str,
    crate_root: &Path,
    crate_name: &str,
    workspace_crates: &HashSet<String>,
) -> ModuleInfo {
    let name = if module.is_crate_root(db) {
        module
            .krate(db)
            .display_name(db)
            .map(|n| normalize_crate_name(n.as_str()))
            .unwrap_or_else(|| "crate".to_string())
    } else {
        module
            .name(db)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "<anonymous>".to_string())
    };

    // Build full path: root is crate name, children are "crate_name::module_name"
    let full_path = if module.is_crate_root(db) {
        parent_path.to_string()
    } else {
        format!("{}::{}", parent_path, name)
    };

    // Extract module dependencies from imports/uses in this module's scope
    let dependencies =
        extract_module_dependencies(module, db, vfs, crate_root, crate_name, workspace_crates);

    let children: Vec<ModuleInfo> = module
        .declarations(db)
        .into_iter()
        .filter_map(|decl| {
            if let hir::ModuleDef::Module(child_module) = decl {
                Some(walk_module(
                    child_module,
                    db,
                    vfs,
                    &full_path,
                    crate_root,
                    crate_name,
                    workspace_crates,
                ))
            } else {
                None
            }
        })
        .collect();

    ModuleInfo {
        name,
        full_path,
        children,
        dependencies,
    }
}

/// Extract module-level dependencies by parsing use statements from source
fn extract_module_dependencies(
    module: hir::Module,
    db: &ide::RootDatabase,
    vfs: &ra_ap_vfs::Vfs,
    crate_root: &Path,
    crate_name: &str,
    workspace_crates: &HashSet<String>,
) -> Vec<DependencyRef> {
    // Get the source file for this module
    let source = module.definition_source(db);
    let editioned_file_id = source.file_id.original_file(db);
    let file_id = editioned_file_id.file_id(db);

    // Get file path from VFS and read from disk
    let vfs_path = vfs.file_path(file_id);
    let Some(abs_path) = vfs_path.as_path() else {
        return Vec::new();
    };
    // Make path relative to crate root
    let abs_path_buf = PathBuf::from(abs_path.as_str());
    let source_file = abs_path_buf
        .strip_prefix(crate_root)
        .map(|p| p.to_path_buf())
        .unwrap_or(abs_path_buf);
    // Graceful degradation: rust-analyzer already parsed this file successfully,
    // so read errors here are rare edge cases (file deleted mid-run, permissions).
    // Missing deps are acceptable - the module still appears, just without edges.
    let source_text = match std::fs::read_to_string(abs_path.as_str()) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Use the new workspace-aware parsing function
    parse_workspace_dependencies(&source_text, crate_name, workspace_crates, &source_file)
}

/// Loads the entire workspace into rust-analyzer once.
/// Returns the AnalysisHost and VFS for reuse across multiple crate analyses.
pub fn load_workspace_hir(
    manifest_path: &Path,
    feature_config: &FeatureConfig,
) -> Result<(ide::AnalysisHost, ra_ap_vfs::Vfs)> {
    let project_path = manifest_path.canonicalize()?;
    let project_path = dunce::simplified(&project_path).to_path_buf();

    // Build cargo config with feature and cfg overrides
    let cargo_config = cargo_config_with_features(feature_config);

    // Load config - minimal for faster loading
    let load_config = load_cargo::LoadCargoConfig {
        load_out_dirs_from_check: false,
        prefill_caches: false,
        with_proc_macro_server: load_cargo::ProcMacroServerChoice::None,
    };

    // Discover project manifest - convert PathBuf -> Utf8PathBuf -> AbsPathBuf
    let utf8_path = paths::Utf8PathBuf::from_path_buf(project_path.clone())
        .map_err(|_| anyhow::anyhow!("Invalid UTF-8 path"))?;
    let root = paths::AbsPathBuf::assert(utf8_path);
    let manifest = project_model::ProjectManifest::discover_single(root.as_path())?;

    // Load project workspace
    let project_workspace =
        project_model::ProjectWorkspace::load(manifest, &cargo_config, &|_| {})?;

    // Load into analysis database
    let (db, vfs, _proc_macro) =
        load_cargo::load_workspace(project_workspace, &Default::default(), &load_config)?;

    let host = ide::AnalysisHost::with_database(db);
    Ok((host, vfs))
}

/// Finds a specific crate in an already-loaded workspace by matching its path.
fn find_crate_in_workspace(
    crate_info: &CrateInfo,
    host: &ide::AnalysisHost,
    vfs: &ra_ap_vfs::Vfs,
) -> Result<hir::Crate> {
    let crate_path = crate_info.path.canonicalize()?;
    let crate_path = dunce::simplified(&crate_path).to_path_buf();
    let crate_utf8 = paths::Utf8PathBuf::from_path_buf(crate_path)
        .map_err(|_| anyhow::anyhow!("Invalid UTF-8 path"))?;
    let crate_dir = paths::AbsPathBuf::assert(crate_utf8);

    let crates = hir::Crate::all(host.raw_database());
    crates
        .into_iter()
        .find(|k| {
            let root_file = k.root_file(host.raw_database());
            let vfs_path = vfs.file_path(root_file);
            vfs_path
                .as_path()
                .map(|p| p.starts_with(&crate_dir))
                .unwrap_or(false)
        })
        .context(format!(
            "Crate '{}' not found in loaded workspace",
            crate_info.name
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_feature_config_default() {
        let config = FeatureConfig::default();
        assert!(config.features.is_empty());
        assert!(!config.all_features);
        assert!(config.cfg_flags.is_empty());
        assert!(!config.no_default_features);
    }

    #[test]
    fn test_feature_config_no_default_features() {
        let config = FeatureConfig {
            no_default_features: true,
            ..Default::default()
        };
        assert!(config.no_default_features);
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
    fn test_cfg_overrides_include_features() {
        let config = FeatureConfig {
            features: vec!["server".to_string()],
            ..Default::default()
        };
        let cargo_config = cargo_config_with_features(&config);

        // CfgDiff Display should show the feature being enabled
        let diff_str = format!("{}", cargo_config.cfg_overrides.global);
        assert!(
            diff_str.contains("feature") && diff_str.contains("server"),
            "Expected feature = \"server\" in cfg_overrides, got: {}",
            diff_str
        );
    }

    #[test]
    fn test_cargo_config_default_excludes_test() {
        let config = FeatureConfig::default();
        let cargo_config = cargo_config_with_features(&config);

        // CfgDiff Display shows "disable test" when test is disabled
        let diff_str = format!("{}", cargo_config.cfg_overrides.global);
        assert!(
            diff_str.contains("disable") && diff_str.contains("test"),
            "Expected cfg(test) to be disabled, got: {}",
            diff_str
        );
    }

    #[test]
    fn test_cargo_config_includes_test_when_flag_set() {
        let config = FeatureConfig {
            cfg_flags: vec!["test".to_string()],
            ..Default::default()
        };
        let cargo_config = cargo_config_with_features(&config);

        // CfgDiff Display shows "enable test" when test is enabled
        let diff_str = format!("{}", cargo_config.cfg_overrides.global);
        assert!(
            diff_str.contains("enable") && diff_str.contains("test"),
            "Expected cfg(test) to be enabled, got: {}",
            diff_str
        );
    }

    #[test]
    fn test_cargo_config_selected_features() {
        let config = FeatureConfig {
            features: vec!["web".to_string()],
            ..Default::default()
        };
        let cargo_config = cargo_config_with_features(&config);

        match cargo_config.features {
            project_model::CargoFeatures::Selected { features, .. } => {
                assert_eq!(features, vec!["web"]);
            }
            _ => panic!("expected Selected"),
        }
    }

    #[test]
    fn test_cargo_features_no_default() {
        let config = FeatureConfig {
            features: vec!["x".to_string()],
            no_default_features: true,
            ..Default::default()
        };
        let cargo_config = cargo_config_with_features(&config);

        match cargo_config.features {
            project_model::CargoFeatures::Selected {
                features,
                no_default_features,
            } => {
                assert_eq!(features, vec!["x"]);
                assert!(no_default_features, "no_default_features should be true");
            }
            _ => panic!("expected Selected"),
        }
    }

    #[test]
    fn test_dependency_ref_struct() {
        let dep = DependencyRef {
            target_crate: "my_crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::from("src/cli.rs"),
            line: 42,
        };
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert!(dep.target_item.is_none());
        assert_eq!(dep.source_file, PathBuf::from("src/cli.rs"));
        assert_eq!(dep.line, 42);
    }

    #[test]
    fn test_dependency_ref_full_target() {
        let dep = DependencyRef {
            target_crate: "crate".to_string(),
            target_module: "graph".to_string(),
            target_item: Some("build".to_string()),
            source_file: PathBuf::new(),
            line: 1,
        };
        assert_eq!(dep.full_target(), "crate::graph::build");
    }

    #[test]
    fn test_dependency_ref_module_target() {
        let dep = DependencyRef {
            target_crate: "crate".to_string(),
            target_module: "graph".to_string(),
            target_item: Some("build".to_string()),
            source_file: PathBuf::new(),
            line: 1,
        };
        assert_eq!(dep.module_target(), "crate::graph");
    }

    #[test]
    fn test_dependency_ref_full_target_no_item() {
        let dep = DependencyRef {
            target_crate: "crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::new(),
            line: 1,
        };
        assert_eq!(dep.full_target(), "crate::graph");
    }

    #[test]
    fn test_module_info_has_dependency_refs() {
        let module = ModuleInfo {
            name: "cli".to_string(),
            full_path: "crate::cli".to_string(),
            children: vec![],
            dependencies: vec![DependencyRef {
                target_crate: "crate".to_string(),
                target_module: "graph".to_string(),
                target_item: None,
                source_file: PathBuf::from("src/cli.rs"),
                line: 5,
            }],
        };
        assert!(
            module
                .dependencies
                .iter()
                .any(|d| d.module_target() == "crate::graph")
        );
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

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_analyze_modules_self() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates = analyze_workspace(&manifest, &FeatureConfig::default())
            .expect("should analyze workspace");
        let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
            .expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates)
            .expect("should analyze modules");

        // cargo-arc root module should be named "cargo_arc"
        assert_eq!(tree.root.name, "cargo_arc");

        // cargo-arc has 4 modules: analyze, graph, layout, render
        let child_names: Vec<_> = tree.root.children.iter().map(|m| m.name.as_str()).collect();
        assert!(
            child_names.contains(&"analyze"),
            "should contain 'analyze' module, found: {:?}",
            child_names
        );
        assert!(
            child_names.contains(&"graph"),
            "should contain 'graph' module, found: {:?}",
            child_names
        );
        assert!(
            child_names.contains(&"layout"),
            "should contain 'layout' module, found: {:?}",
            child_names
        );
        assert!(
            child_names.contains(&"render"),
            "should contain 'render' module, found: {:?}",
            child_names
        );
    }

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_module_full_path() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates = analyze_workspace(&manifest, &FeatureConfig::default())
            .expect("should analyze workspace");
        let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
            .expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates)
            .expect("should analyze modules");

        // Root module full_path should be the normalized crate name
        assert_eq!(tree.root.full_path, "cargo_arc");

        // Child modules should have full paths like "cargo_arc::analyze"
        let analyze_module = tree
            .root
            .children
            .iter()
            .find(|m| m.name == "analyze")
            .expect("should find analyze module");
        assert_eq!(analyze_module.full_path, "cargo_arc::analyze");
    }

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_module_dependencies() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates = analyze_workspace(&manifest, &FeatureConfig::default())
            .expect("should analyze workspace");
        let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
            .expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates)
            .expect("should analyze modules");

        // graph module depends on model (use crate::model::{...})
        let graph_module = tree
            .root
            .children
            .iter()
            .find(|m| m.name == "graph")
            .expect("should find graph module");
        assert!(
            graph_module
                .dependencies
                .iter()
                .any(|d| d.module_target() == "cargo_arc::model"),
            "graph should depend on model, found: {:?}",
            graph_module.dependencies
        );

        // cli module depends on analyze, graph, layout, render
        let cli_module = tree
            .root
            .children
            .iter()
            .find(|m| m.name == "cli")
            .expect("should find cli module");
        assert!(
            cli_module
                .dependencies
                .iter()
                .any(|d| d.module_target() == "cargo_arc::analyze"),
            "cli should depend on analyze, found: {:?}",
            cli_module.dependencies
        );
        assert!(
            cli_module
                .dependencies
                .iter()
                .any(|d| d.module_target() == "cargo_arc::graph"),
            "cli should depend on graph, found: {:?}",
            cli_module.dependencies
        );

        // render module depends on layout
        let render_module = tree
            .root
            .children
            .iter()
            .find(|m| m.name == "render")
            .expect("should find render module");
        assert!(
            render_module
                .dependencies
                .iter()
                .any(|d| d.module_target() == "cargo_arc::layout"),
            "render should depend on layout, found: {:?}",
            render_module.dependencies
        );
    }

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
        let mut resolved_deps: std::collections::HashMap<&str, Vec<String>> =
            std::collections::HashMap::new();
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
        let mut resolved_deps: std::collections::HashMap<&str, Vec<String>> =
            std::collections::HashMap::new();
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
        let mut resolved_deps: std::collections::HashMap<&str, Vec<String>> =
            std::collections::HashMap::new();
        resolved_deps.insert("A", vec!["B".to_string()]);
        resolved_deps.insert("B", vec!["A".to_string()]);
        let workspace: HashSet<&str> = ["A", "B"].into_iter().collect();

        let reachable = collect_reachable_crates(seeds, &resolved_deps, &workspace);

        assert!(reachable.contains("A"));
        assert!(reachable.contains("B"));
        assert_eq!(reachable.len(), 2);
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
