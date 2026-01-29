//! HIR-based module analysis using rust-analyzer.

use super::use_parser::{normalize_crate_name, parse_workspace_dependencies};
use crate::model::{CrateInfo, DependencyRef, ModuleInfo, ModuleTree};

#[derive(Debug, Clone, Default)]
pub struct FeatureConfig {
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub cfg_flags: Vec<String>,
    pub debug: bool,
}

use anyhow::{Context, Result};
use ra_ap_cfg::{CfgAtom, CfgDiff};
use ra_ap_hir as hir;
use ra_ap_ide as ide;
use ra_ap_load_cargo as load_cargo;
use ra_ap_paths as paths;
use ra_ap_project_model as project_model;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
pub(crate) fn find_crate_in_workspace(
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

/// Resolves a module's display name and full path.
/// Root modules use the crate's display name; child modules use their declared name.
fn resolve_module_name_and_path(
    module: hir::Module,
    db: &ide::RootDatabase,
    parent_path: &str,
) -> (String, String) {
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

    let full_path = if module.is_crate_root(db) {
        parent_path.to_string()
    } else {
        format!("{}::{}", parent_path, name)
    };

    (name, full_path)
}

/// Collects all module paths from hir::Module tree (lightweight, no dependency analysis).
/// Returns relative paths without crate prefix, e.g. {"analyze", "analyze::use_parser"}.
pub(crate) fn collect_hir_module_paths(
    module: hir::Module,
    db: &ide::RootDatabase,
    parent_path: &str,
    crate_name: &str,
) -> HashSet<String> {
    let mut result = HashSet::new();
    collect_module_paths_recursive(module, db, parent_path, crate_name, &mut result);
    result
}

fn collect_module_paths_recursive(
    module: hir::Module,
    db: &ide::RootDatabase,
    parent_path: &str,
    crate_name: &str,
    result: &mut HashSet<String>,
) {
    let (_name, full_path) = resolve_module_name_and_path(module, db, parent_path);

    // Add relative path (without crate prefix) for non-root modules
    if !module.is_crate_root(db) {
        let prefix = format!("{}::", crate_name);
        if let Some(relative) = full_path.strip_prefix(&prefix) {
            result.insert(relative.to_string());
        }
    }

    for decl in module.declarations(db) {
        if let hir::ModuleDef::Module(child_module) = decl {
            collect_module_paths_recursive(child_module, db, &full_path, crate_name, result);
        }
    }
}

/// Analyzes the module hierarchy of a crate using rust-analyzer's HIR.
/// The `host` and `vfs` should be obtained from `load_workspace_hir()`.
/// `workspace_crates` should contain all workspace crate names for inter-crate dependency detection.
pub fn analyze_modules(
    crate_info: &CrateInfo,
    host: &ide::AnalysisHost,
    vfs: &ra_ap_vfs::Vfs,
    workspace_crates: &HashSet<String>,
    all_module_paths: &HashMap<String, HashSet<String>>,
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
        all_module_paths,
    );

    Ok(ModuleTree { root })
}

#[allow(clippy::too_many_arguments)]
fn walk_module(
    module: hir::Module,
    db: &ide::RootDatabase,
    vfs: &ra_ap_vfs::Vfs,
    parent_path: &str,
    crate_root: &Path,
    crate_name: &str,
    workspace_crates: &HashSet<String>,
    all_module_paths: &HashMap<String, HashSet<String>>,
) -> ModuleInfo {
    let (name, full_path) = resolve_module_name_and_path(module, db, parent_path);

    // Extract module dependencies from imports/uses in this module's scope
    let dependencies = extract_module_dependencies(
        module,
        db,
        vfs,
        crate_root,
        crate_name,
        workspace_crates,
        all_module_paths,
    );

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
                    all_module_paths,
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
    all_module_paths: &HashMap<String, HashSet<String>>,
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
    parse_workspace_dependencies(
        &source_text,
        crate_name,
        workspace_crates,
        &source_file,
        all_module_paths,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_ap_project_model as project_model;

    mod unit_tests {
        use super::*;

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
        fn test_cfg_overrides_include_features() {
            let config = FeatureConfig {
                features: vec!["server".to_string()],
                ..Default::default()
            };
            let cargo_config = cargo_config_with_features(&config);

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
    }

    mod smoke_tests {
        use super::*;
        use crate::analyze::workspace::analyze_workspace;

        #[test]
        #[ignore] // Smoke test - requires rust-analyzer (~30s)
        fn test_collect_hir_module_paths() {
            let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
            let crates = analyze_workspace(&manifest, &FeatureConfig::default())
                .expect("should analyze workspace");
            let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

            let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
                .expect("should load workspace");
            let krate = find_crate_in_workspace(cargo_arc, &host, &vfs).expect("should find crate");
            let db = host.raw_database();
            let crate_name = normalize_crate_name(&cargo_arc.name);
            let paths =
                collect_hir_module_paths(krate.root_module(db), db, &crate_name, &crate_name);

            assert!(
                paths.contains("analyze"),
                "should contain 'analyze', found: {:?}",
                paths
            );
            assert!(
                paths.contains("analyze::hir"),
                "should contain 'analyze::hir', found: {:?}",
                paths
            );
            assert!(
                paths.contains("analyze::use_parser"),
                "should contain 'analyze::use_parser', found: {:?}",
                paths
            );
            // Must NOT contain crate prefix
            assert!(
                !paths.iter().any(|p| p.starts_with("cargo_arc::")),
                "paths should be relative (no crate prefix), found: {:?}",
                paths
            );
        }

        #[test]
        #[ignore] // Smoke test - requires rust-analyzer (~30s)
        fn test_analyze_modules_self() {
            let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
            let crates = analyze_workspace(&manifest, &FeatureConfig::default())
                .expect("should analyze workspace");
            let workspace_crates: std::collections::HashSet<String> =
                crates.iter().map(|c| c.name.clone()).collect();
            let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

            let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
                .expect("should load workspace");
            let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates, &HashMap::new())
                .expect("should analyze modules");

            assert_eq!(tree.root.name, "cargo_arc");

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
            let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
            let crates = analyze_workspace(&manifest, &FeatureConfig::default())
                .expect("should analyze workspace");
            let workspace_crates: std::collections::HashSet<String> =
                crates.iter().map(|c| c.name.clone()).collect();
            let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

            let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
                .expect("should load workspace");
            let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates, &HashMap::new())
                .expect("should analyze modules");

            assert_eq!(tree.root.full_path, "cargo_arc");

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
            let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
            let crates = analyze_workspace(&manifest, &FeatureConfig::default())
                .expect("should analyze workspace");
            let workspace_crates: std::collections::HashSet<String> =
                crates.iter().map(|c| c.name.clone()).collect();
            let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

            let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
                .expect("should load workspace");
            let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates, &HashMap::new())
                .expect("should analyze modules");

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
    }
}
