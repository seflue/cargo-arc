use anyhow::Result;
use clap::Parser;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use crate::analyze::{
    AnalysisBackend, FeatureConfig, analyze_workspace, collect_crate_exports, normalize_crate_name,
};
use crate::graph::build_graph;
use crate::layout::{build_layout, detect_cycles, topo_sort};
use crate::model::{CrateExportMap, ModulePathMap, WorkspaceCrates};
use crate::render::{RenderConfig, render};
use crate::volatility::{VolatilityAnalyzer, VolatilityConfig};
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

    /// Include test code in analysis (unit tests, integration tests)
    #[arg(long)]
    pub include_tests: bool,

    /// Enable debug output to stderr (shows filtering decisions)
    #[arg(long)]
    pub debug: bool,

    /// Print volatility report (text) instead of dependency SVG
    #[arg(long)]
    pub volatility: bool,

    /// Disable git volatility analysis in SVG output
    #[arg(long)]
    pub no_volatility: bool,

    /// Volatility analysis period in months (default: 6)
    #[arg(long, default_value = "6")]
    pub volatility_months: usize,

    /// Low volatility threshold (default: 2)
    #[arg(long, default_value = "2")]
    pub volatility_low: usize,

    /// High volatility threshold (default: 10)
    #[arg(long, default_value = "10")]
    pub volatility_high: usize,

    /// Use rust-analyzer HIR backend instead of syn (slower but may catch more)
    #[cfg(feature = "hir")]
    #[arg(long)]
    pub hir: bool,
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

    let vol_config = VolatilityConfig {
        months: args.volatility_months,
        low_threshold: args.volatility_low,
        high_threshold: args.volatility_high,
    };

    // Volatility-only mode: skip the full pipeline, just run git analysis
    if args.volatility {
        let repo_path = args
            .manifest_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut analyzer = VolatilityAnalyzer::new(vol_config);
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
        include_tests: args.include_tests,
        debug: args.debug,
    };

    // 2. Analyze workspace with feature config
    let crates = analyze_workspace(&args.manifest_path, &feature_config)?;

    // 3. Build workspace crate names set for inter-crate dependency detection
    let workspace_crates: WorkspaceCrates = crates.iter().map(|c| c.name.clone()).collect();

    // 4. Create analysis backend (syn default, hir only with --hir flag)
    #[cfg(feature = "hir")]
    let use_hir = args.hir;
    #[cfg(not(feature = "hir"))]
    let use_hir = false;
    let backend = AnalysisBackend::new(&args.manifest_path, &feature_config, use_hir)?;

    // 5a. Collect module paths from ALL crates
    let all_module_paths: ModulePathMap = crates
        .iter()
        .map(|c| {
            let name = normalize_crate_name(&c.name);
            let paths = backend.collect_module_paths(c);
            (name, paths)
        })
        .collect();

    // 5a2. Collect crate exports for entry-point detection
    let crate_exports: CrateExportMap = crates
        .iter()
        .map(|c| {
            let name = normalize_crate_name(&c.name);
            let exports = collect_crate_exports(&c.path);
            (name, exports)
        })
        .collect();

    // 5b. Analyze modules for each crate
    let modules: Vec<_> = crates
        .iter()
        .filter_map(|c| {
            backend
                .analyze_modules(c, &workspace_crates, &all_module_paths, &crate_exports)
                .ok()
        })
        .collect();

    // 6. Build dependency graph
    let graph = build_graph(&crates, &modules);

    // 7. Detect cycles
    let cycles = detect_cycles(&graph);

    // 8. Topological sort
    let order = topo_sort(&graph, &cycles);

    // 9. Build layout (CrateDep edges skipped when ModuleDeps exist between crates)
    let mut layout = build_layout(&graph, &order, &cycles);

    // 9b. Populate volatility data (graceful degradation on failure)
    if !args.no_volatility {
        let repo_path = args
            .manifest_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut analyzer = VolatilityAnalyzer::new(vol_config);
        match analyzer.analyze(repo_path) {
            Ok(()) => {
                for item in &mut layout.items {
                    if let Some(ref path) = item.source_path {
                        let vol = analyzer.get_volatility(path);
                        let count = analyzer.get_change_count(path);
                        item.volatility = Some((vol, count));
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Volatility analysis skipped: {e}");
            }
        }
    }

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
    fn test_cli_include_tests_flag() {
        let args = parse_args(&["cargo", "arc", "--include-tests"]);
        assert!(args.include_tests);
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
    fn test_cli_no_volatility_flag() {
        let args = parse_args(&["cargo", "arc", "--no-volatility"]);
        assert!(args.no_volatility);
    }

    #[test]
    fn test_cli_volatility_months() {
        let args = parse_args(&["cargo", "arc", "--volatility-months", "3"]);
        assert_eq!(args.volatility_months, 3);
    }

    #[test]
    fn test_cli_volatility_thresholds() {
        let args = parse_args(&[
            "cargo",
            "arc",
            "--volatility-low",
            "5",
            "--volatility-high",
            "20",
        ]);
        assert_eq!(args.volatility_low, 5);
        assert_eq!(args.volatility_high, 20);
    }

    #[test]
    fn test_cli_volatility_config_defaults() {
        let args = parse_args(&["cargo", "arc"]);
        assert!(!args.no_volatility);
        assert_eq!(args.volatility_months, 6);
        assert_eq!(args.volatility_low, 2);
        assert_eq!(args.volatility_high, 10);
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
            include_tests: false,
            debug: false,
            volatility: false,
            no_volatility: false,
            volatility_months: 6,
            volatility_low: 2,
            volatility_high: 10,
            #[cfg(feature = "hir")]
            hir: false,
        };
        let result = run(args);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(temp.path()).unwrap();
        assert!(content.contains("<svg"));
    }
}
