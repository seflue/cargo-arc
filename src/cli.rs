use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use crate::analyze::{
    AnalysisBackend, FeatureConfig, ReExportMap, analyze_workspace, collect_crate_exports,
    collect_crate_reexports, externals::analyze_externals, normalize_crate_name,
};
use crate::diagnose::MinimalCycles;
use crate::graph::ArcGraph;
use crate::layout::{LayoutIR, build_layout};
use crate::model::{CrateExportMap, ModulePathMap, WorkspaceCrates};
use crate::render::{RenderConfig, render};
use crate::rules::config::{ArcConfig, ConfigError};
use crate::rules::engine::{CycleCluster, check_rules};
use crate::rules::format::{format_cluster_report, format_violations};
use crate::volatility::{VolatilityAnalyzer, VolatilityConfig};
use std::path::Path;

/// Cargo subcommand wrapper for `cargo arc`
#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
pub enum Cargo {
    /// Visualize workspace dependencies as SVG or check architecture rules
    #[command(name = "arc", version, author)]
    Arc(ArcCommand),
}

#[allow(clippy::struct_excessive_bools)] // CLI flags map 1:1 to fields
#[derive(Parser)]
pub struct ArcCommand {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Validate dependency graph (exit 1 if cycles found) [legacy, use `check` subcommand]
    #[arg(long, hide = true)]
    pub check: bool,

    /// Output file (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

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

    /// Include external crate dependencies in visualization
    #[arg(long)]
    pub externals: bool,

    /// Include transitive external dependencies (requires --externals)
    #[arg(long)]
    pub transitive_deps: bool,

    /// Initial expand level for SVG (0=crates only, 1=direct modules, etc.)
    #[arg(long)]
    pub expand_level: Option<usize>,

    /// Use rust-analyzer HIR backend instead of syn (slower but may catch more)
    #[cfg(feature = "hir")]
    #[arg(long)]
    pub hir: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Check architecture rules against dependency graph
    Check(CheckArgs),
}

#[derive(Parser)]
pub struct CheckArgs {
    /// Path to rules file (default: arc-rules.toml in workspace root)
    #[arg(long)]
    pub rules: Option<PathBuf>,
}

/// Shared flags for analysis configuration, used by both diagram and check modes.
#[allow(clippy::struct_excessive_bools)] // CLI flags map 1:1 to fields
#[derive(Parser)]
pub struct CommonArgs {
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

    /// Include pure re-export (`pub use`) cycles in cycle analysis. Off by
    /// default: such cycles are idiomatic republishing, not real coupling.
    #[arg(long)]
    pub include_reexports: bool,

    /// Enable debug output to stderr (shows filtering decisions)
    #[arg(long)]
    pub debug: bool,
}

#[allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
pub fn run(args: ArcCommand) -> Result<()> {
    if args.common.debug {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::from_default_env().add_directive("cargo_arc=debug".parse().unwrap()),
            )
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }

    // Handle `check` subcommand or legacy `--check` flag
    match args.command {
        Some(Command::Check(check_args)) => {
            return run_check(&check_args, &args.common);
        }
        None if args.check => {
            let check_args = CheckArgs { rules: None };
            return run_check(&check_args, &args.common);
        }
        None => {}
    }

    let vol_config = VolatilityConfig {
        months: args.volatility_months,
        low_threshold: args.volatility_low,
        high_threshold: args.volatility_high,
    };

    if args.volatility {
        return run_volatility_report(&args.common.manifest_path, vol_config, args.output.as_ref());
    }

    let feature_config = build_feature_config(&args.common);

    #[cfg(feature = "hir")]
    let use_hir = args.hir;
    #[cfg(not(feature = "hir"))]
    let use_hir = false;

    let graph = build_dependency_graph(
        &args.common.manifest_path,
        &feature_config,
        use_hir,
        args.externals,
        args.transitive_deps,
    )?;
    tracing::debug!("phase: cycle detection start");
    let analysis = graph
        .cycle_subgraph(args.common.include_reexports)
        .minimal_cycles();
    tracing::debug!(
        "phase: cycle detection done ({} cycles)",
        analysis.cycles.len()
    );
    let mut layout = build_layout(&graph, &analysis, args.common.include_reexports);
    tracing::debug!("phase: layout built ({} items)", layout.items.len());

    if !args.no_volatility {
        enrich_volatility(&mut layout, &args.common.manifest_path, vol_config);
    }

    let config = RenderConfig {
        expand_level: args.expand_level,
        ..RenderConfig::default()
    };
    let svg = render(&layout, &config);
    tracing::debug!("phase: render done ({} bytes)", svg.len());
    write_output(&svg, args.output.as_ref())
}

/// Run the `check` subcommand: load rules, evaluate against graph, report violations.
fn run_check(check_args: &CheckArgs, common: &CommonArgs) -> Result<()> {
    let feature_config = build_feature_config(common);

    #[cfg(feature = "hir")]
    let use_hir = false; // check mode doesn't support HIR
    #[cfg(not(feature = "hir"))]
    let use_hir = false;

    let graph = build_dependency_graph(
        &common.manifest_path,
        &feature_config,
        use_hir,
        false,
        false,
    )?;

    let workspace_root = resolve_repo_path(&common.manifest_path);
    let default_rules_path = workspace_root.join("arc-rules.toml");
    let rules_path = check_args.rules.as_deref().unwrap_or(&default_rules_path);
    let explicit = check_args.rules.is_some();

    let config = match ArcConfig::load(rules_path) {
        Ok(config) => config,
        Err(ConfigError::FileNotFound(..)) if !explicit => {
            // No arc-rules.toml and not explicitly requested → legacy cycle check
            return run_legacy_cycle_check(&graph, common.include_reexports);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    tracing::debug!("phase: rule check start");
    let result = check_rules(&graph, &config, common.include_reexports);
    tracing::debug!(
        "phase: rule check done ({} violations)",
        result.violations.len()
    );
    if !result.violations.is_empty() {
        eprint!("{}", format_violations(&result));
    }

    let code = result.exit_code();
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

/// Legacy fallback: global cycle check when no arc-rules.toml exists.
fn run_legacy_cycle_check(graph: &ArcGraph, include_reexports: bool) -> Result<()> {
    tracing::debug!("phase: cycle detection start (--check)");
    let sub = graph.cycle_subgraph(include_reexports);
    let analysis = sub.minimal_cycles();
    tracing::debug!(
        "phase: cycle detection done ({} cycles)",
        analysis.cycles.len()
    );
    if analysis.cycles.is_empty() {
        return Ok(());
    }
    let report = graph.cluster_report(&sub, &analysis);
    let total = report.clusters.len();
    let clusters: Vec<CycleCluster> = report
        .clusters
        .iter()
        .enumerate()
        .map(|(i, cluster)| CycleCluster::from_cluster(graph, &analysis, cluster, i + 1, total))
        .collect();
    eprint!("{}", format_cluster_report(&clusters));
    anyhow::bail!("dependency cycle(s) detected");
}

fn build_feature_config(common: &CommonArgs) -> FeatureConfig {
    FeatureConfig {
        features: common.features.clone(),
        all_features: common.all_features,
        no_default_features: common.no_default_features,
        include_tests: common.include_tests,
        debug: common.debug,
    }
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
    externals: bool,
    transitive_deps: bool,
) -> Result<ArcGraph> {
    let crates = analyze_workspace(manifest_path, feature_config)?;
    tracing::debug!("phase: workspace analyzed ({} crates)", crates.len());
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
    tracing::debug!("phase: module paths collected");

    let crate_exports: CrateExportMap = crates
        .iter()
        .map(|krate| {
            let name = normalize_crate_name(&krate.name);
            let exports = collect_crate_exports(krate);
            (name, exports)
        })
        .collect();
    tracing::debug!("phase: crate exports collected");

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
    tracing::debug!("phase: reexport map collected");

    // Run externals analysis before module analysis so crate_name_map
    // is available for use-parser resolution of external crate imports.
    let ext_result = if externals {
        use cargo_metadata::MetadataCommand;
        let metadata = MetadataCommand::new().manifest_path(manifest_path).exec()?;
        Some(analyze_externals(&metadata, transitive_deps))
    } else {
        None
    };

    let empty_name_map = std::collections::HashMap::new();
    let modules: Vec<_> = crates
        .iter()
        .filter_map(|krate| {
            let name = normalize_crate_name(&krate.name);
            tracing::debug!("analyzing crate: {name}");
            let ext_names = ext_result
                .as_ref()
                .and_then(|r| r.crate_name_map.get(&name))
                .unwrap_or(&empty_name_map);
            match backend.analyze_modules(
                krate,
                &workspace_crates,
                &all_module_paths,
                &crate_exports,
                &reexport_map,
                ext_names,
            ) {
                Ok(tree) => Some(tree),
                Err(err) => {
                    tracing::warn!("Skipping crate {}: {err}", krate.name);
                    None
                }
            }
        })
        .collect();
    tracing::debug!("phase: all crates analyzed");

    let graph = ArcGraph::build(
        &crates,
        &modules,
        ext_result.as_ref(),
        feature_config.include_tests,
    );
    tracing::debug!(
        "phase: graph built ({} nodes, {} edges)",
        graph.node_count(),
        graph.edge_count()
    );
    Ok(graph)
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

    /// Helper to parse ArcCommand via Cargo wrapper
    fn parse_args(args: &[&str]) -> ArcCommand {
        let Cargo::Arc(cmd) = Cargo::parse_from(args);
        cmd
    }

    // ===== Task 3.2: check subcommand parsing tests =====

    #[test]
    fn test_parse_check_subcommand() {
        let cmd = parse_args(&["cargo", "arc", "check"]);
        assert!(matches!(cmd.command, Some(Command::Check(ref args)) if args.rules.is_none()));
    }

    #[test]
    fn test_parse_check_with_rules() {
        let cmd = parse_args(&["cargo", "arc", "check", "--rules", "x.toml"]);
        match cmd.command {
            Some(Command::Check(ref args)) => {
                assert_eq!(args.rules, Some(PathBuf::from("x.toml")));
            }
            _ => panic!("expected Command::Check"),
        }
    }

    #[test]
    fn test_parse_legacy_check_flag() {
        let cmd = parse_args(&["cargo", "arc", "--check"]);
        assert!(cmd.check);
        assert!(cmd.command.is_none());
    }

    #[test]
    fn test_parse_diagram_default() {
        let cmd = parse_args(&["cargo", "arc"]);
        assert!(cmd.command.is_none());
        assert!(!cmd.check);
    }

    #[test]
    fn test_parse_common_args_on_check() {
        // Common args must come before the subcommand
        let cmd = parse_args(&["cargo", "arc", "--features", "web", "check"]);
        assert!(matches!(cmd.command, Some(Command::Check(..))));
        assert_eq!(cmd.common.features, vec!["web"]);
    }

    #[test]
    fn test_parse_check_flag_plus_subcommand() {
        // --check + check subcommand: subcommand takes precedence
        let cmd = parse_args(&["cargo", "arc", "--check", "check"]);
        assert!(matches!(cmd.command, Some(Command::Check(..))));
    }

    // ===== Legacy CLI parsing tests (adapted from old Args) =====

    #[test]
    fn test_cli_default_args() {
        let cmd = parse_args(&["cargo", "arc"]);
        assert!(cmd.output.is_none());
        assert_eq!(cmd.common.manifest_path, PathBuf::from("Cargo.toml"));
    }

    #[test]
    fn test_cli_features_parsing() {
        let cmd = parse_args(&["cargo", "arc", "--features", "web,server"]);
        assert_eq!(cmd.common.features, vec!["web", "server"]);
    }

    #[test]
    fn test_cli_all_features() {
        let cmd = parse_args(&["cargo", "arc", "--all-features"]);
        assert!(cmd.common.all_features);
    }

    #[test]
    fn test_cli_include_tests_flag() {
        let cmd = parse_args(&["cargo", "arc", "--include-tests"]);
        assert!(cmd.common.include_tests);
    }

    #[test]
    fn test_cli_no_default_features_flag() {
        let cmd = parse_args(&["cargo", "arc", "--no-default-features"]);
        assert!(cmd.common.no_default_features);
    }

    #[test]
    fn test_cli_volatility_flag() {
        let cmd = parse_args(&["cargo", "arc", "--volatility"]);
        assert!(cmd.volatility);
    }

    #[test]
    fn test_cli_no_volatility_flag() {
        let cmd = parse_args(&["cargo", "arc", "--no-volatility"]);
        assert!(cmd.no_volatility);
    }

    #[test]
    fn test_cli_volatility_months() {
        let cmd = parse_args(&["cargo", "arc", "--volatility-months", "3"]);
        assert_eq!(cmd.volatility_months, 3);
    }

    #[test]
    fn test_cli_volatility_thresholds() {
        let cmd = parse_args(&[
            "cargo",
            "arc",
            "--volatility-low",
            "5",
            "--volatility-high",
            "20",
        ]);
        assert_eq!(cmd.volatility_low, 5);
        assert_eq!(cmd.volatility_high, 20);
    }

    #[test]
    fn test_parse_externals_flag() {
        let cmd = parse_args(&["cargo", "arc", "--externals"]);
        assert!(cmd.externals);
    }

    #[test]
    fn test_parse_externals_flag_default() {
        let cmd = parse_args(&["cargo", "arc"]);
        assert!(!cmd.externals);
    }

    #[test]
    fn test_parse_transitive_deps_flag() {
        let cmd = parse_args(&["cargo", "arc", "--externals", "--transitive-deps"]);
        assert!(cmd.externals);
        assert!(cmd.transitive_deps);
    }

    #[test]
    fn test_parse_transitive_deps_flag_default() {
        let cmd = parse_args(&["cargo", "arc"]);
        assert!(!cmd.transitive_deps);
    }

    #[test]
    fn test_parse_expand_level() {
        let cmd = parse_args(&["cargo", "arc", "--expand-level", "0"]);
        assert_eq!(cmd.expand_level, Some(0));
    }

    #[test]
    fn test_parse_expand_level_two() {
        let cmd = parse_args(&["cargo", "arc", "--expand-level", "2"]);
        assert_eq!(cmd.expand_level, Some(2));
    }

    #[test]
    fn test_parse_expand_level_default() {
        let cmd = parse_args(&["cargo", "arc"]);
        assert!(cmd.expand_level.is_none());
    }

    #[test]
    fn test_cli_volatility_config_defaults() {
        let cmd = parse_args(&["cargo", "arc"]);
        assert!(!cmd.no_volatility);
        assert_eq!(cmd.volatility_months, 6);
        assert_eq!(cmd.volatility_low, 2);
        assert_eq!(cmd.volatility_high, 10);
    }

    #[test]
    #[ignore] // Smoke test - requires rust-analyzer (~30s)
    fn test_run_with_output_file() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let cmd = ArcCommand {
            command: None,
            common: CommonArgs {
                manifest_path: PathBuf::from("Cargo.toml"),
                features: vec![],
                all_features: false,
                no_default_features: false,
                include_tests: false,
                include_reexports: false,
                debug: false,
            },
            check: false,
            output: Some(temp.path().to_path_buf()),
            volatility: false,
            no_volatility: false,
            volatility_months: 6,
            volatility_low: 2,
            volatility_high: 10,
            externals: false,
            transitive_deps: false,
            expand_level: None,
            #[cfg(feature = "hir")]
            hir: false,
        };
        let result = run(cmd);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(temp.path()).unwrap();
        assert!(content.contains("<svg"));
    }
}
