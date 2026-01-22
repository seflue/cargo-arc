//! Workspace & Module Analysis

use anyhow::{Context, Result};
use cargo_metadata::{MetadataCommand, Package};
use ra_ap_hir as hir;
use ra_ap_ide as ide;
use ra_ap_load_cargo as load_cargo;
use ra_ap_paths as paths;
use ra_ap_project_model as project_model;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    pub path: PathBuf,
    pub dependencies: Vec<String>,
}

/// Analyzes a workspace and returns all member crates.
/// `manifest_path` should point to a Cargo.toml.
pub fn analyze_workspace(manifest_path: &Path) -> Result<Vec<CrateInfo>> {
    let metadata = MetadataCommand::new()
        .manifest_path(manifest_path)
        .exec()
        .context("Failed to run cargo metadata")?;

    // Collect workspace member names for dependency filtering
    let workspace_members: HashSet<&str> = metadata
        .workspace_packages()
        .iter()
        .map(|p| p.name.as_str())
        .collect();

    let crates: Vec<CrateInfo> = metadata
        .workspace_packages()
        .into_iter()
        .map(|pkg| package_to_crate_info(pkg, &workspace_members))
        .collect();

    Ok(crates)
}

fn package_to_crate_info(pkg: &Package, workspace_members: &HashSet<&str>) -> CrateInfo {
    use cargo_metadata::DependencyKind;

    let dependencies: Vec<String> = pkg
        .dependencies
        .iter()
        // Only normal dependencies (exclude dev and build deps to avoid false cycles)
        .filter(|dep| dep.kind == DependencyKind::Normal)
        .filter(|dep| workspace_members.contains(dep.name.as_str()))
        .map(|dep| dep.name.clone())
        .collect();

    CrateInfo {
        name: pkg.name.clone(),
        path: pkg.manifest_path.parent().unwrap().into(),
        dependencies,
    }
}

// ============================================================================
// Crate Name Utilities
// ============================================================================

/// Normalizes a crate name to its canonical form (hyphens -> underscores).
/// Cargo crates with hyphens in their name appear as underscores in Rust code.
fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

// ============================================================================
// Module Hierarchy Analysis (via ra_ap_hir)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct DependencyRef {
    pub target_crate: String,
    pub target_module: String,
    pub target_item: Option<String>,
    pub source_file: PathBuf,
    pub line: usize,
}

impl DependencyRef {
    /// Returns full target path: "crate::module::item" or "crate::module" if no item.
    pub fn full_target(&self) -> String {
        match &self.target_item {
            Some(item) => format!("{}::{}::{}", self.target_crate, self.target_module, item),
            None => format!("{}::{}", self.target_crate, self.target_module),
        }
    }

    /// Returns module-level target: "crate::module" (ignores item).
    pub fn module_target(&self) -> String {
        format!("{}::{}", self.target_crate, self.target_module)
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub full_path: String,
    pub children: Vec<ModuleInfo>,
    pub dependencies: Vec<DependencyRef>,
}

#[derive(Debug, Clone)]
pub struct ModuleTree {
    pub root: ModuleInfo,
}

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
    let root_module = krate.root_module();
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
    let name = if module.is_crate_root() {
        module
            .krate()
            .display_name(db)
            .map(|n| n.to_string().replace('-', "_"))
            .unwrap_or_else(|| "crate".to_string())
    } else {
        module
            .name(db)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "<anonymous>".to_string())
    };

    // Build full path: root is "crate", children are "crate::module_name"
    let full_path = if module.is_crate_root() {
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

/// Process a single use statement line, returning a DependencyRef if it's a relevant import.
///
/// Handles:
/// - `use crate::module;` - crate-local imports
/// - `use crate::module::item;` - crate-local item imports
/// - `use workspace_crate::module;` - workspace crate imports (when in workspace_crates set)
///
/// Returns None for:
/// - `use self::*` or `use super::*` - relative imports
/// - External crate imports (not in workspace_crates)
#[allow(dead_code)] // Will be used by parse_workspace_dependencies
fn process_use_statement(
    line: &str,
    line_num: usize,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
) -> Option<DependencyRef> {
    let line = line.trim();
    if !line.starts_with("use ") {
        return None;
    }

    // Extract the path after "use "
    let path = line.strip_prefix("use ")?.trim_end_matches(';').trim();

    // Handle crate-local imports: use crate::module[::item]
    // Use "crate" as target_crate to match module_map keys ("crate::module")
    if let Some(after_crate) = path.strip_prefix("crate::") {
        let parts: Vec<&str> = after_crate.split("::").collect();
        if parts.is_empty() {
            return None;
        }
        // First part is module, rest is item (if any)
        let module = parts[0].trim_end_matches('{').trim();
        if module.is_empty() {
            return None;
        }
        let item = if parts.len() > 1 {
            let item_part = parts[1].trim_end_matches('{').trim();
            if item_part.is_empty() || item_part.starts_with('{') {
                None
            } else {
                Some(item_part.to_string())
            }
        } else {
            None
        };

        // Use actual crate name (normalized) for consistent module_map lookup
        return Some(DependencyRef {
            target_crate: normalize_crate_name(current_crate),
            target_module: module.to_string(),
            target_item: item,
            source_file: source_file.to_path_buf(),
            line: line_num,
        });
    }

    // Handle workspace crate imports: use other_crate::module[::item]
    // The first segment is the crate name (may have underscores, Cargo.toml may have hyphens)
    let parts: Vec<&str> = path.split("::").collect();
    if parts.is_empty() {
        return None;
    }

    let first_segment = parts[0].trim();

    // Check if this is a workspace crate (normalize both sides for comparison)
    let normalized_first = normalize_crate_name(first_segment);
    let is_workspace_crate = workspace_crates
        .iter()
        .any(|ws_crate| normalize_crate_name(ws_crate) == normalized_first);

    if is_workspace_crate && parts.len() >= 2 {
        let module = parts[1].trim_end_matches('{').trim_end_matches(';').trim();
        if module.is_empty() {
            return None;
        }
        let item = if parts.len() > 2 {
            let item_part = parts[2].trim_end_matches('{').trim_end_matches(';').trim();
            if item_part.is_empty() {
                None
            } else {
                Some(item_part.to_string())
            }
        } else {
            None
        };

        return Some(DependencyRef {
            target_crate: first_segment.to_string(),
            target_module: module.to_string(),
            target_item: item,
            source_file: source_file.to_path_buf(),
            line: line_num,
        });
    }

    None
}

/// Parse use statements from source code, extracting workspace-relevant dependencies.
///
/// Returns DependencyRefs for:
/// - Crate-local imports (`use crate::module`)
/// - Workspace crate imports (`use other_crate::module` where other_crate is in workspace)
///
/// Deduplicates by module_target() keeping first occurrence.
fn parse_workspace_dependencies(
    source: &str,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashSet<String> = HashSet::new();

    for (line_idx, line) in source.lines().enumerate() {
        let line_num = line_idx + 1;
        if let Some(dep) =
            process_use_statement(line, line_num, current_crate, workspace_crates, source_file)
        {
            let target_key = dep.module_target();
            if !seen_targets.contains(&target_key) {
                seen_targets.insert(target_key);
                deps.push(dep);
            }
        }
    }

    deps
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
    let source_text = match std::fs::read_to_string(abs_path.as_str()) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Use the new workspace-aware parsing function
    parse_workspace_dependencies(&source_text, crate_name, workspace_crates, &source_file)
}

/// Loads the entire workspace into rust-analyzer once.
/// Returns the AnalysisHost and VFS for reuse across multiple crate analyses.
pub fn load_workspace_hir(manifest_path: &Path) -> Result<(ide::AnalysisHost, ra_ap_vfs::Vfs)> {
    let project_path = manifest_path.canonicalize()?;
    let project_path = dunce::simplified(&project_path).to_path_buf();

    // Minimal cargo config
    let cargo_config = project_model::CargoConfig {
        sysroot: Some(project_model::RustLibSource::Discover),
        ..Default::default()
    };

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

    // ========================================================================
    // normalize_crate_name() tests
    // ========================================================================

    #[test]
    fn test_normalize_crate_name() {
        assert_eq!(normalize_crate_name("my-lib"), "my_lib");
        assert_eq!(normalize_crate_name("already_valid"), "already_valid");
        assert_eq!(normalize_crate_name("a-b-c"), "a_b_c");
    }

    // ========================================================================
    // process_use_statement() tests
    // ========================================================================

    #[test]
    fn test_process_use_statement_crate_local() {
        let ws: HashSet<String> = HashSet::new();
        let dep = process_use_statement(
            "use crate::graph::build;",
            1,
            "my_crate",
            &ws,
            Path::new("src/cli.rs"),
        );
        let dep = dep.expect("should parse crate-local import");
        // Crate-local imports use actual crate name for inter-crate module_map lookup
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert_eq!(dep.target_item, Some("build".to_string()));
    }

    #[test]
    fn test_process_use_statement_crate_local_module_only() {
        let ws: HashSet<String> = HashSet::new();
        let dep = process_use_statement(
            "use crate::graph;",
            5,
            "my_crate",
            &ws,
            Path::new("src/lib.rs"),
        );
        let dep = dep.expect("should parse crate-local module import");
        // Crate-local imports use actual crate name for inter-crate module_map lookup
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert!(dep.target_item.is_none());
        assert_eq!(dep.line, 5);
    }

    #[test]
    fn test_process_use_statement_workspace_crate() {
        let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
        let dep = process_use_statement(
            "use other_crate::utils;",
            1,
            "my_crate",
            &ws,
            Path::new("src/lib.rs"),
        );
        let dep = dep.expect("should parse workspace crate import");
        assert_eq!(dep.target_crate, "other_crate");
        assert_eq!(dep.target_module, "utils");
    }

    #[test]
    fn test_process_use_statement_workspace_crate_with_hyphen() {
        // Crate name has hyphen in Cargo.toml but appears as underscore in use statement
        let ws: HashSet<String> = HashSet::from(["my-lib".to_string()]);
        let dep = process_use_statement(
            "use my_lib::feature;",
            1,
            "app",
            &ws,
            Path::new("src/main.rs"),
        );
        let dep = dep.expect("should parse workspace crate with hyphen");
        assert_eq!(dep.target_crate, "my_lib");
        assert_eq!(dep.target_module, "feature");
    }

    #[test]
    fn test_process_use_statement_relative_self_ignored() {
        let ws: HashSet<String> = HashSet::new();
        let dep = process_use_statement(
            "use self::helper;",
            1,
            "my_crate",
            &ws,
            Path::new("src/lib.rs"),
        );
        assert!(dep.is_none(), "self:: imports should be ignored");
    }

    #[test]
    fn test_process_use_statement_relative_super_ignored() {
        let ws: HashSet<String> = HashSet::new();
        let dep = process_use_statement(
            "use super::parent;",
            1,
            "my_crate",
            &ws,
            Path::new("src/sub/mod.rs"),
        );
        assert!(dep.is_none(), "super:: imports should be ignored");
    }

    #[test]
    fn test_process_use_statement_external_filtered() {
        let ws: HashSet<String> = HashSet::from(["my_crate".to_string()]);
        let dep = process_use_statement(
            "use serde::Serialize;",
            1,
            "my_crate",
            &ws,
            Path::new("src/lib.rs"),
        );
        assert!(dep.is_none(), "external crate imports should be filtered");
    }

    #[test]
    fn test_process_use_statement_std_filtered() {
        let ws: HashSet<String> = HashSet::new();
        let dep = process_use_statement(
            "use std::collections::HashMap;",
            1,
            "my_crate",
            &ws,
            Path::new("src/lib.rs"),
        );
        assert!(dep.is_none(), "std imports should be filtered");
    }

    // ========================================================================
    // parse_workspace_dependencies() tests
    // ========================================================================

    #[test]
    fn test_parse_workspace_dependencies_mixed() {
        let source = r#"
use crate::graph;
use other_crate::utils;
use serde::Serialize;
use std::collections::HashMap;
"#;
        let ws: HashSet<String> = HashSet::from(["my_crate".into(), "other_crate".into()]);
        let deps = parse_workspace_dependencies(source, "my_crate", &ws, Path::new("src/lib.rs"));

        // Should have 2 deps: my_crate::graph and other_crate::utils
        assert_eq!(deps.len(), 2, "found: {:?}", deps);
        // Both crate-local and workspace crates use actual crate names
        assert!(
            deps.iter()
                .any(|d| d.target_crate == "my_crate" && d.target_module == "graph")
        );
        assert!(
            deps.iter()
                .any(|d| d.target_crate == "other_crate" && d.target_module == "utils")
        );
    }

    #[test]
    fn test_parse_workspace_dependencies_dedup_by_module() {
        let source = r#"
use crate::graph::build;
use crate::graph::Node;
use crate::graph;
"#;
        let ws: HashSet<String> = HashSet::new();
        let deps = parse_workspace_dependencies(source, "my_crate", &ws, Path::new("src/cli.rs"));

        // Should deduplicate by module_target
        assert_eq!(deps.len(), 1, "should deduplicate by target module");
        assert_eq!(deps[0].target_module, "graph");
        assert_eq!(deps[0].line, 2, "should keep first occurrence");
    }

    #[test]
    fn test_analyze_workspace_self() {
        // Test with cargo-arc itself as workspace
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates = analyze_workspace(&manifest).expect("should analyze");

        // cargo-arc should find itself
        assert!(!crates.is_empty());
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc");
        assert!(cargo_arc.is_some(), "should find cargo-arc");
    }

    #[test]
    fn test_crate_info_fields() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates = analyze_workspace(&manifest).expect("should analyze");

        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();
        assert!(cargo_arc.path.exists(), "path should exist");
        // dependencies is empty because cargo-arc has no workspace-internal deps
        // (only external: clap, petgraph, etc.)
    }

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_analyze_modules_self() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates = analyze_workspace(&manifest).expect("should analyze workspace");
        let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest).expect("should load workspace");
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
        let crates = analyze_workspace(&manifest).expect("should analyze workspace");
        let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest).expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates)
            .expect("should analyze modules");

        // Root module full_path should be "crate"
        assert_eq!(tree.root.full_path, "crate");

        // Child modules should have full paths like "crate::analyze"
        let analyze_module = tree
            .root
            .children
            .iter()
            .find(|m| m.name == "analyze")
            .expect("should find analyze module");
        assert_eq!(analyze_module.full_path, "crate::analyze");
    }

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_module_dependencies() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let crates = analyze_workspace(&manifest).expect("should analyze workspace");
        let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest).expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs, &workspace_crates)
            .expect("should analyze modules");

        // graph module depends on analyze (use crate::analyze::{...})
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
                .any(|d| d.module_target() == "crate::analyze"),
            "graph should depend on analyze, found: {:?}",
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
                .any(|d| d.module_target() == "crate::analyze"),
            "cli should depend on analyze, found: {:?}",
            cli_module.dependencies
        );
        assert!(
            cli_module
                .dependencies
                .iter()
                .any(|d| d.module_target() == "crate::graph"),
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
                .any(|d| d.module_target() == "crate::layout"),
            "render should depend on layout, found: {:?}",
            render_module.dependencies
        );
    }
}
