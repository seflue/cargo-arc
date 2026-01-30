use anyhow::Result;
use clap::Parser;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use crate::analyze::{
    FeatureConfig, analyze_modules, analyze_workspace, collect_hir_module_paths,
    find_crate_in_workspace, load_workspace_hir, normalize_crate_name,
};
use crate::graph::build_graph;
use crate::layout::{build_layout, detect_cycles, topo_sort};
use crate::render::{RenderConfig, render};
use crate::volatility::{VolatilityAnalyzer, VolatilityConfig};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Cargo subcommand wrapper for `cargo arc`
#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
pub enum Cargo {
    /// Visualize workspace dependencies as SVG
    #[command(name = "arc", version, author)]
    Arc(Args),
}

#[derive(Parser)]
pub struct Args {
    /// Output file (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Path to Cargo.toml (default: ./Cargo.toml)
    #[arg(short, long, default_value = "Cargo.toml")]
    pub manifest_path: PathBuf,

    /// Comma-separated list of features to activate
    #[arg(long, value_delimiter = ',')]
    pub features: Vec<String>,

    /// Activate all available features
    #[arg(long)]
    pub all_features: bool,

    /// Do not activate the `default` feature
    #[arg(long)]
    pub no_default_features: bool,

    /// Cfg flag to enable (e.g., --cfg test to include test code)
    #[arg(long = "cfg", value_name = "CFG")]
    pub cfg: Vec<String>,

    /// Enable debug output to stderr (shows filtering decisions)
    #[arg(long)]
    pub debug: bool,

    /// Print volatility report (text) instead of dependency SVG
    #[arg(long)]
    pub volatility: bool,
}

pub fn run(args: Args) -> Result<()> {
    // Initialize tracing if debug mode is enabled
    if args.debug {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::from_default_env().add_directive("cargo_arc=debug".parse().unwrap()),
            )
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }

    // Volatility-only mode: skip the full pipeline, just run git analysis
    if args.volatility {
        let repo_path = args
            .manifest_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig::default());
        analyzer.analyze(repo_path)?;
        let report = analyzer.format_report();
        match args.output {
            Some(path) => fs::write(&path, &report)?,
            None => io::stdout().write_all(report.as_bytes())?,
        }
        return Ok(());
    }

    // 1. Build feature config from CLI args (needed for both analyze_workspace and load_workspace_hir)
    let feature_config = FeatureConfig {
        features: args.features,
        all_features: args.all_features,
        no_default_features: args.no_default_features,
        cfg_flags: args.cfg,
        debug: args.debug,
    };

    // 2. Analyze workspace with feature config
    let crates = analyze_workspace(&args.manifest_path, &feature_config)?;

    // 3. Build workspace crate names set for inter-crate dependency detection
    let workspace_crates: HashSet<String> = crates.iter().map(|c| c.name.clone()).collect();

    // 4. Load rust-analyzer ONCE for the entire workspace
    let (host, vfs) = load_workspace_hir(&args.manifest_path, &feature_config)?;

    // 5a. Collect module paths from ALL crates (lightweight hir walk)
    let db = host.raw_database();
    let all_module_paths: HashMap<String, HashSet<String>> = crates
        .iter()
        .filter_map(|c| {
            let krate = find_crate_in_workspace(c, &host, &vfs).ok()?;
            let name = normalize_crate_name(&c.name);
            let paths = collect_hir_module_paths(krate.root_module(db), db, &name, &name);
            Some((name, paths))
        })
        .collect();

    // 5b. Analyze modules for each crate (reusing loaded workspace)
    let modules: Vec<_> = crates
        .iter()
        .filter_map(|c| analyze_modules(c, &host, &vfs, &workspace_crates, &all_module_paths).ok())
        .collect();

    // 6. Build dependency graph
    let graph = build_graph(&crates, &modules);

    // 7. Detect cycles
    let cycles = detect_cycles(&graph);

    // 8. Topological sort
    let order = topo_sort(&graph, &cycles);

    // 9. Build layout (CrateDep edges skipped when ModuleDeps exist between crates)
    let layout = build_layout(&graph, &order, &cycles);

    // 10. Render to SVG
    let svg = render(&layout, &RenderConfig::default());

    // 11. Output
    match args.output {
        Some(path) => fs::write(&path, &svg)?,
        None => io::stdout().write_all(svg.as_bytes())?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse Args via Cargo wrapper
    fn parse_args(args: &[&str]) -> Args {
        let Cargo::Arc(args) = Cargo::parse_from(args);
        args
    }

    #[test]
    fn test_cli_default_args() {
        let args = parse_args(&["cargo", "arc"]);
        assert!(args.output.is_none());
        assert_eq!(args.manifest_path, PathBuf::from("Cargo.toml"));
    }

    #[test]
    fn test_cli_features_parsing() {
        let args = parse_args(&["cargo", "arc", "--features", "web,server"]);
        assert_eq!(args.features, vec!["web", "server"]);
    }

    #[test]
    fn test_cli_all_features() {
        let args = parse_args(&["cargo", "arc", "--all-features"]);
        assert!(args.all_features);
    }

    #[test]
    fn test_cli_cfg_flag() {
        let args = parse_args(&["cargo", "arc", "--cfg", "test"]);
        assert_eq!(args.cfg, vec!["test"]);
    }

    #[test]
    fn test_cli_no_default_features_flag() {
        let args = parse_args(&["cargo", "arc", "--no-default-features"]);
        assert!(args.no_default_features);
    }

    #[test]
    fn test_cli_volatility_flag() {
        let args = parse_args(&["cargo", "arc", "--volatility"]);
        assert!(args.volatility);
    }

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_run_with_output_file() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let args = Args {
            output: Some(temp.path().to_path_buf()),
            manifest_path: PathBuf::from("Cargo.toml"),
            features: vec![],
            all_features: false,
            no_default_features: false,
            cfg: vec![],
            debug: false,
            volatility: false,
        };
        let result = run(args);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(temp.path()).unwrap();
        assert!(content.contains("<svg"));
    }
}
