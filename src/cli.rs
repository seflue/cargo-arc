use anyhow::Result;
use clap::Parser;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use crate::analyze::{
    AnalysisBackend, FeatureConfig, ReExportMap, analyze_workspace, collect_crate_exports,
    collect_crate_reexports, externals::analyze_externals, normalize_crate_name,
};
use crate::graph::ArcGraph;
use crate::layout::{Cycle, ElementaryCycles, LayoutIR, build_layout};
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

    /// Validate dependency graph (exit 1 if cycles found)
    #[arg(long)]
    pub check: bool,

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

    if args.check {
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

        let graph = build_dependency_graph(
            &args.manifest_path,
            &feature_config,
            use_hir,
            args.externals,
            args.transitive_deps,
        )?;
        tracing::debug!("phase: cycle detection start (--check)");
        let cycles = graph.production_subgraph().elementary_cycles();
        tracing::debug!("phase: cycle detection done ({} cycles)", cycles.len());
        if cycles.is_empty() {
            return Ok(());
        }
        eprint!("{}", format_cycle_errors(&graph, &cycles));
        anyhow::bail!("dependency cycle(s) detected");
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

    let graph = build_dependency_graph(
        &args.manifest_path,
        &feature_config,
        use_hir,
        args.externals,
        args.transitive_deps,
    )?;
    tracing::debug!("phase: cycle detection start");
    let cycles = graph.production_subgraph().elementary_cycles();
    tracing::debug!("phase: cycle detection done ({} cycles)", cycles.len());
    let mut layout = build_layout(&graph, &cycles);
    tracing::debug!("phase: layout built ({} items)", layout.items.len());

    if !args.no_volatility {
        enrich_volatility(&mut layout, &args.manifest_path, vol_config);
    }

    let config = RenderConfig {
        expand_level: args.expand_level,
        ..RenderConfig::default()
    };
    let svg = render(&layout, &config);
    tracing::debug!("phase: render done ({} bytes)", svg.len());
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
            let exports = collect_crate_exports(&krate.path);
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

    let graph = ArcGraph::build(&crates, &modules, ext_result.as_ref());
    tracing::debug!(
        "phase: graph built ({} nodes, {} edges)",
        graph.node_count(),
        graph.edge_count()
    );
    Ok(graph)
}

/// Format detected cycles as compiler-style error messages.
///
/// Returns an empty string when `cycles` is empty. Otherwise produces one
/// `error[cycle]:` line per cycle (using `<->` for direct / `->` chains for
/// transitive) followed by a summary line.
fn format_cycle_errors(graph: &ArcGraph, cycles: &[Cycle]) -> String {
    use std::fmt::Write;

    if cycles.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for cycle in cycles {
        let names: Vec<&str> = cycle.path.iter().map(|&idx| graph[idx].name()).collect();
        if names.len() == 2 {
            let _ = writeln!(output, "error[cycle]: {} <-> {}", names[0], names[1]);
        } else {
            let _ = writeln!(
                output,
                "error[cycle]: {} -> {}",
                names.join(" -> "),
                names[0]
            );
        }
    }
    let _ = write!(
        output,
        "\nerror: found {} cycle(s) in dependency graph\n",
        cycles.len()
    );
    output
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

    use crate::graph::Node;
    use crate::layout::Cycle;
    use petgraph::graph::NodeIndex;

    /// Build a test graph with named module nodes.
    fn test_graph(names: &[&str]) -> (ArcGraph, Vec<NodeIndex>) {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "test".into(),
            path: "/test".into(),
        });
        let indices: Vec<_> = names
            .iter()
            .map(|name| {
                graph.add_node(Node::Module {
                    name: (*name).into(),
                    crate_idx,
                })
            })
            .collect();
        (graph, indices)
    }

    #[test]
    fn test_parse_check_flag() {
        let args = parse_args(&["cargo", "arc", "--check"]);
        assert!(args.check);
    }

    #[test]
    fn test_parse_check_flag_default() {
        let args = parse_args(&["cargo", "arc"]);
        assert!(!args.check);
    }

    #[test]
    fn test_format_cycle_errors_transitive() {
        let (graph, idx) = test_graph(&["A", "B", "C"]);
        let cycles = vec![Cycle {
            path: vec![idx[0], idx[1], idx[2]],
        }];
        let output = format_cycle_errors(&graph, &cycles);
        assert!(output.contains("error[cycle]: A -> B -> C -> A"));
    }

    #[test]
    fn test_format_cycle_errors_direct() {
        let (graph, idx) = test_graph(&["A", "B"]);
        let cycles = vec![Cycle {
            path: vec![idx[0], idx[1]],
        }];
        let output = format_cycle_errors(&graph, &cycles);
        assert!(output.contains("error[cycle]: A <-> B"));
    }

    #[test]
    fn test_format_cycle_errors_empty() {
        let (graph, _) = test_graph(&["A", "B"]);
        let output = format_cycle_errors(&graph, &[]);
        assert!(output.is_empty());
    }

    #[test]
    fn test_format_cycle_errors_summary() {
        let (graph, idx) = test_graph(&["A", "B", "C", "D"]);
        let cycles = vec![
            Cycle {
                path: vec![idx[0], idx[1]],
            },
            Cycle {
                path: vec![idx[2], idx[3]],
            },
        ];
        let output = format_cycle_errors(&graph, &cycles);
        assert!(output.contains("error: found 2 cycle(s) in dependency graph"));
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
    fn test_parse_externals_flag() {
        let args = parse_args(&["cargo", "arc", "--externals"]);
        assert!(args.externals);
    }

    #[test]
    fn test_parse_externals_flag_default() {
        let args = parse_args(&["cargo", "arc"]);
        assert!(!args.externals);
    }

    #[test]
    fn test_parse_transitive_deps_flag() {
        let args = parse_args(&["cargo", "arc", "--externals", "--transitive-deps"]);
        assert!(args.externals);
        assert!(args.transitive_deps);
    }

    #[test]
    fn test_parse_transitive_deps_flag_default() {
        let args = parse_args(&["cargo", "arc"]);
        assert!(!args.transitive_deps);
    }

    #[test]
    fn test_parse_expand_level() {
        let args = parse_args(&["cargo", "arc", "--expand-level", "0"]);
        assert_eq!(args.expand_level, Some(0));
    }

    #[test]
    fn test_parse_expand_level_two() {
        let args = parse_args(&["cargo", "arc", "--expand-level", "2"]);
        assert_eq!(args.expand_level, Some(2));
    }

    #[test]
    fn test_parse_expand_level_default() {
        let args = parse_args(&["cargo", "arc"]);
        assert!(args.expand_level.is_none());
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
            check: false,
            debug: false,
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
        let result = run(args);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(temp.path()).unwrap();
        assert!(content.contains("<svg"));
    }
}
