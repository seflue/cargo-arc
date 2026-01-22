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
// Module Hierarchy Analysis (via ra_ap_hir)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct DependencyRef {
    pub target: String,
    pub source_file: PathBuf,
    pub line: usize,
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
pub fn analyze_modules(
    crate_info: &CrateInfo,
    host: &ide::AnalysisHost,
    vfs: &ra_ap_vfs::Vfs,
) -> Result<ModuleTree> {
    // Find crate in already-loaded workspace
    let krate = find_crate_in_workspace(crate_info, host, vfs)?;
    let db = host.raw_database();

    // Walk module tree starting from crate root
    let root_module = krate.root_module();
    let root = walk_module(root_module, db, vfs, "crate", &crate_info.path);

    Ok(ModuleTree { root })
}

fn walk_module(
    module: hir::Module,
    db: &ide::RootDatabase,
    vfs: &ra_ap_vfs::Vfs,
    parent_path: &str,
    crate_root: &Path,
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
    let dependencies = extract_module_dependencies(module, db, vfs, crate_root);

    let children: Vec<ModuleInfo> = module
        .declarations(db)
        .into_iter()
        .filter_map(|decl| {
            if let hir::ModuleDef::Module(child_module) = decl {
                Some(walk_module(child_module, db, vfs, &full_path, crate_root))
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

/// Parse use statements from source code, extracting crate-internal dependencies.
/// Returns a list of (target module path, line number) pairs.
///
/// Matches patterns like:
/// - `use crate::foo;`
/// - `use crate::foo::bar;`
/// - `use crate::foo::{...};`
pub fn parse_crate_dependencies(source: &str) -> Vec<(String, usize)> {
    let mut deps: Vec<(String, usize)> = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        let line_num = line_idx + 1; // 1-indexed
        let line = line.trim();
        if !line.starts_with("use crate::") {
            continue;
        }

        // Extract the first module segment after "crate::"
        // "use crate::analyze::{...}" -> "analyze"
        // "use crate::graph::build_graph" -> "graph"
        let after_crate = &line["use crate::".len()..];
        if let Some(module_name) = after_crate.split("::").next() {
            // Clean up: remove trailing ; or {
            let module_name = module_name
                .trim_end_matches(';')
                .trim_end_matches('{')
                .trim();
            if !module_name.is_empty() {
                let target = format!("crate::{}", module_name);
                // Only add if we don't already have this target (dedup by target)
                if !deps.iter().any(|(t, _)| t == &target) {
                    deps.push((target, line_num));
                }
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

    // Use the pure parsing function and convert to DependencyRef
    parse_crate_dependencies(&source_text)
        .into_iter()
        .map(|(target, line)| DependencyRef {
            target,
            source_file: source_file.clone(),
            line,
        })
        .collect()
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
            target: "crate::graph".to_string(),
            source_file: PathBuf::from("src/cli.rs"),
            line: 42,
        };
        assert_eq!(dep.target, "crate::graph");
        assert_eq!(dep.source_file, PathBuf::from("src/cli.rs"));
        assert_eq!(dep.line, 42);
    }

    #[test]
    fn test_module_info_has_dependency_refs() {
        let module = ModuleInfo {
            name: "cli".to_string(),
            full_path: "crate::cli".to_string(),
            children: vec![],
            dependencies: vec![DependencyRef {
                target: "crate::graph".to_string(),
                source_file: PathBuf::from("src/cli.rs"),
                line: 5,
            }],
        };
        assert!(
            module
                .dependencies
                .iter()
                .any(|d| d.target == "crate::graph")
        );
    }

    // ========================================================================
    // parse_crate_dependencies() unit tests - fast, no rust-analyzer
    // ========================================================================

    #[test]
    fn test_parse_crate_dependencies_simple() {
        let source = r#"
use crate::graph;
use crate::analyze;
"#;
        let deps = parse_crate_dependencies(source);
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|(t, _)| t == "crate::graph"));
        assert!(deps.iter().any(|(t, _)| t == "crate::analyze"));
    }

    #[test]
    fn test_parse_crate_dependencies_nested() {
        let source = r#"
use crate::analyze::{CrateInfo, ModuleInfo};
use crate::graph::build_graph;
use crate::layout::{build_layout, detect_cycles};
"#;
        let deps = parse_crate_dependencies(source);
        assert_eq!(deps.len(), 3);
        assert!(deps.iter().any(|(t, _)| t == "crate::analyze"));
        assert!(deps.iter().any(|(t, _)| t == "crate::graph"));
        assert!(deps.iter().any(|(t, _)| t == "crate::layout"));
    }

    #[test]
    fn test_parse_crate_dependencies_dedup() {
        let source = r#"
use crate::graph::build_graph;
use crate::graph::Node;
use crate::graph;
"#;
        let deps = parse_crate_dependencies(source);
        assert_eq!(deps.len(), 1, "should deduplicate by target");
        assert_eq!(deps[0].0, "crate::graph");
        assert_eq!(deps[0].1, 2, "should keep first occurrence line number");
    }

    #[test]
    fn test_parse_crate_dependencies_line_numbers() {
        let source = r#"// Comment
use std::path::Path;
use crate::foo;
// Another comment
use crate::bar;
"#;
        let deps = parse_crate_dependencies(source);
        assert_eq!(deps.len(), 2);
        let foo = deps.iter().find(|(t, _)| t == "crate::foo").unwrap();
        let bar = deps.iter().find(|(t, _)| t == "crate::bar").unwrap();
        assert_eq!(foo.1, 3, "crate::foo on line 3");
        assert_eq!(bar.1, 5, "crate::bar on line 5");
    }

    #[test]
    fn test_parse_crate_dependencies_ignores_external() {
        let source = r#"
use std::collections::HashMap;
use anyhow::Result;
use crate::analyze;
use petgraph::Graph;
"#;
        let deps = parse_crate_dependencies(source);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].0, "crate::analyze");
    }

    #[test]
    fn test_parse_crate_dependencies_empty() {
        let source = r#"
// No use statements
fn main() {}
"#;
        let deps = parse_crate_dependencies(source);
        assert!(deps.is_empty());
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
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest).expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs).expect("should analyze modules");

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
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest).expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs).expect("should analyze modules");

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
        let cargo_arc = crates.iter().find(|c| c.name == "cargo-arc").unwrap();

        let (host, vfs) = load_workspace_hir(&manifest).expect("should load workspace");
        let tree = analyze_modules(cargo_arc, &host, &vfs).expect("should analyze modules");

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
                .any(|d| d.target == "crate::analyze"),
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
                .any(|d| d.target == "crate::analyze"),
            "cli should depend on analyze, found: {:?}",
            cli_module.dependencies
        );
        assert!(
            cli_module
                .dependencies
                .iter()
                .any(|d| d.target == "crate::graph"),
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
                .any(|d| d.target == "crate::layout"),
            "render should depend on layout, found: {:?}",
            render_module.dependencies
        );
    }
}
