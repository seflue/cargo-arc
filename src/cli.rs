use anyhow::Result;
use clap::Parser;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use crate::analyze::{
    AnalysisBackend, FeatureConfig, ReExportMap, analyze_workspace, collect_crate_exports,
    collect_crate_reexports, normalize_crate_name,
};
use crate::graph::ArcGraph;
use crate::layout::{ElementaryCycles, LayoutIR, build_layout};
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

#[allow(clippy::struct_excessive_bools)] // CLI flags map 1:1 to fields
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

#[allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
pub fn run(args: Args) -> Result<()> {
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

    if args.volatility {
        return run_volatility_report(&args.manifest_path, vol_config, args.output.as_ref());
    }

    let feature_config = FeatureConfig {
        features: args.features,
        all_features: args.all_features,
        no_default_features: args.no_default_features,
        include_tests: args.include_tests,
        debug: args.debug,
    };

    #[cfg(feature = "hir")]
    let use_hir = args.hir;
    #[cfg(not(feature = "hir"))]
    let use_hir = false;

    let graph = build_dependency_graph(&args.manifest_path, &feature_config, use_hir)?;
    let cycles = graph.production_subgraph().elementary_cycles();
    let mut layout = build_layout(&graph, &cycles);

    if !args.no_volatility {
        enrich_volatility(&mut layout, &args.manifest_path, vol_config);
    }

    let svg = render(&layout, &RenderConfig::default());
    write_output(&svg, args.output.as_ref())
}

fn resolve_repo_path(manifest_path: &Path) -> &Path {
    manifest_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn write_output(content: &str, output: Option<&PathBuf>) -> Result<()> {
    match output {
        Some(path) => fs::write(path, content)?,
        None => io::stdout().write_all(content.as_bytes())?,
    }
    Ok(())
}

fn run_volatility_report(
    manifest_path: &Path,
    vol_config: VolatilityConfig,
    output: Option<&PathBuf>,
) -> Result<()> {
    let repo_path = resolve_repo_path(manifest_path);
    let mut analyzer = VolatilityAnalyzer::new(vol_config);
    analyzer.analyze(repo_path)?;
    let report = analyzer.format_report();
    write_output(&report, output)
}

fn build_dependency_graph(
    manifest_path: &Path,
    feature_config: &FeatureConfig,
    use_hir: bool,
) -> Result<ArcGraph> {
    let crates = analyze_workspace(manifest_path, feature_config)?;
    let workspace_crates: WorkspaceCrates = crates.iter().map(|krate| krate.name.clone()).collect();
    let backend = AnalysisBackend::new(manifest_path, feature_config, use_hir)?;

    let all_module_paths: ModulePathMap = crates
        .iter()
        .map(|krate| {
            let name = normalize_crate_name(&krate.name);
            let paths = backend.collect_module_paths(krate);
            (name, paths)
        })
        .collect();

    let crate_exports: CrateExportMap = crates
        .iter()
        .map(|krate| {
            let name = normalize_crate_name(&krate.name);
            let exports = collect_crate_exports(&krate.path);
            (name, exports)
        })
        .collect();

    let reexport_map: ReExportMap = crates
        .iter()
        .map(|krate| {
            let name = normalize_crate_name(&krate.name);
            let exports = collect_crate_reexports(
                krate,
                &all_module_paths,
                &workspace_crates,
                &crate_exports,
            );
            (name, exports)
        })
        .collect();

    let modules: Vec<_> = crates
        .iter()
        .filter_map(|krate| {
            match backend.analyze_modules(
                krate,
                &workspace_crates,
                &all_module_paths,
                &crate_exports,
                &reexport_map,
            ) {
                Ok(tree) => Some(tree),
                Err(err) => {
                    tracing::warn!("Skipping crate {}: {err}", krate.name);
                    None
                }
            }
        })
        .collect();

    Ok(ArcGraph::build(&crates, &modules))
}

fn enrich_volatility(layout: &mut LayoutIR, manifest_path: &Path, vol_config: VolatilityConfig) {
    let repo_path = resolve_repo_path(manifest_path);
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
        Err(err) => {
            tracing::warn!("Volatility analysis skipped: {err}");
        }
    }
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
