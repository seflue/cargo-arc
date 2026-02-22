//! HIR-based module analysis using rust-analyzer.
//!
//! Only `FeatureConfig` is always available. All HIR functions require `feature = "hir"`.

// FeatureConfig is always available (no ra_ap_* dependency)
#[derive(Debug, Clone, Default)]
pub struct FeatureConfig {
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub include_tests: bool,
    pub debug: bool,
}

#[cfg(feature = "hir")]
use {
    super::use_parser::{ResolutionContext, parse_workspace_dependencies_from_source},
    crate::model::normalize_crate_name,
    crate::model::{
        CrateExportMap, CrateInfo, DependencyRef, ModuleInfo, ModulePathMap, ModuleTree,
        WorkspaceCrates,
    },
    anyhow::{Context, Result},
    ra_ap_cfg::{CfgAtom, CfgDiff},
    ra_ap_hir as hir, ra_ap_ide as ide, ra_ap_load_cargo as load_cargo, ra_ap_paths as paths,
    ra_ap_project_model as project_model,
    std::collections::HashSet,
    std::path::{Path, PathBuf},
};

/// Creates a CargoConfig with feature and cfg overrides.
/// By default, cfg(test) is disabled.
#[cfg(feature = "hir")]
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

    let include_test = config.include_tests;
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
#[cfg(feature = "hir")]
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
#[cfg(feature = "hir")]
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
#[cfg(feature = "hir")]
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
#[cfg(feature = "hir")]
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

#[cfg(feature = "hir")]
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
#[cfg(feature = "hir")]
pub fn analyze_modules(
    crate_info: &CrateInfo,
    host: &ide::AnalysisHost,
    vfs: &ra_ap_vfs::Vfs,
    workspace_crates: &WorkspaceCrates,
    all_module_paths: &ModulePathMap,
    crate_exports: &CrateExportMap,
) -> Result<ModuleTree> {
    // Find crate in already-loaded workspace
    let krate = find_crate_in_workspace(crate_info, host, vfs)?;
    let db = host.raw_database();

    // Walk module tree starting from crate root
    let root_module = krate.root_module(db);
    let crate_name = &crate_info.name;
    // Use actual crate name (normalized) as root path for inter-crate dependency resolution
    let normalized_crate_name = normalize_crate_name(crate_name);
    let ctx = HirWalkContext {
        db,
        vfs,
        crate_root: &crate_info.path,
        crate_name,
        workspace_crates,
        all_module_paths,
        crate_exports,
    };
    let root = walk_module(root_module, &normalized_crate_name, &ctx);

    Ok(ModuleTree { root })
}

/// Invariant parameters shared across the recursive HIR module walk.
#[cfg(feature = "hir")]
struct HirWalkContext<'a> {
    db: &'a ide::RootDatabase,
    vfs: &'a ra_ap_vfs::Vfs,
    crate_root: &'a Path,
    crate_name: &'a str,
    workspace_crates: &'a WorkspaceCrates,
    all_module_paths: &'a ModulePathMap,
    crate_exports: &'a CrateExportMap,
}

#[cfg(feature = "hir")]
fn walk_module(module: hir::Module, parent_path: &str, ctx: &HirWalkContext) -> ModuleInfo {
    let (name, full_path) = resolve_module_name_and_path(module, ctx.db, parent_path);

    // Relative module path within the crate (e.g. "render" for render/mod.rs, "" for root)
    let current_module_path = full_path
        .strip_prefix(&format!("{}::", ctx.crate_name))
        .unwrap_or("");

    let dependencies = extract_module_dependencies(module, current_module_path, ctx);

    let children: Vec<ModuleInfo> = module
        .declarations(ctx.db)
        .into_iter()
        .filter_map(|decl| {
            if let hir::ModuleDef::Module(child_module) = decl {
                Some(walk_module(child_module, &full_path, ctx))
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
#[cfg(feature = "hir")]
fn extract_module_dependencies(
    module: hir::Module,
    current_module_path: &str,
    ctx: &HirWalkContext,
) -> Vec<DependencyRef> {
    // Get the source file for this module
    let source = module.definition_source(ctx.db);
    let editioned_file_id = source.file_id.original_file(ctx.db);
    let file_id = editioned_file_id.file_id(ctx.db);

    // Get file path from VFS and read from disk
    let vfs_path = ctx.vfs.file_path(file_id);
    let Some(abs_path) = vfs_path.as_path() else {
        return Vec::new();
    };
    // Make path relative to crate root
    let abs_path_buf = PathBuf::from(abs_path.as_str());
    let source_file = abs_path_buf
        .strip_prefix(ctx.crate_root)
        .map(|p| p.to_path_buf())
        .unwrap_or(abs_path_buf);
    // Graceful degradation: rust-analyzer already parsed this file successfully,
    // so read errors here are rare edge cases (file deleted mid-run, permissions).
    // Missing deps are acceptable - the module still appears, just without edges.
    let source_text = match std::fs::read_to_string(abs_path.as_str()) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let res_ctx = ResolutionContext {
        current_crate: ctx.crate_name,
        workspace_crates: ctx.workspace_crates,
        source_file: &source_file,
        all_module_paths: ctx.all_module_paths,
        crate_exports: ctx.crate_exports,
        current_module_path,
    };
    parse_workspace_dependencies_from_source(&source_text, &res_ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_config_default() {
        let config = FeatureConfig::default();
        assert!(config.features.is_empty());
        assert!(!config.all_features);
        assert!(!config.include_tests);
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
}

#[cfg(all(test, feature = "hir"))]
mod hir_tests {
    use super::*;
    use ra_ap_project_model as project_model;

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
            include_tests: true,
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
            let workspace_crates: WorkspaceCrates = crates.iter().map(|c| c.name.clone()).collect();
            let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

            let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
                .expect("should load workspace");
            let tree = analyze_modules(
                cargo_arc,
                &host,
                &vfs,
                &workspace_crates,
                &ModulePathMap::default(),
                &CrateExportMap::default(),
            )
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
            let workspace_crates: WorkspaceCrates = crates.iter().map(|c| c.name.clone()).collect();
            let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

            let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
                .expect("should load workspace");
            let tree = analyze_modules(
                cargo_arc,
                &host,
                &vfs,
                &workspace_crates,
                &ModulePathMap::default(),
                &CrateExportMap::default(),
            )
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
            let workspace_crates: WorkspaceCrates = crates.iter().map(|c| c.name.clone()).collect();
            let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

            let (host, vfs) = load_workspace_hir(&manifest, &FeatureConfig::default())
                .expect("should load workspace");
            let tree = analyze_modules(
                cargo_arc,
                &host,
                &vfs,
                &workspace_crates,
                &ModulePathMap::default(),
                &CrateExportMap::default(),
            )
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
