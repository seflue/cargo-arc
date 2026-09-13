use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

use crate::analyze::{
    AnalysisBackend, FeatureConfig, ReExportMap, analyze_workspace, collect_crate_exports,
    collect_crate_reexports, externals::analyze_externals, normalize_crate_name,
};
use crate::diagnose::RepresentativeCycles;
use crate::graph::{ArcGraph, Reexports};
use crate::layout::{JumpTable, LayoutIR, build_layout};
use crate::model::{CrateExportMap, CrateInfo, ModulePathMap, WorkspaceCrates};
use crate::render::{RenderConfig, render};
use crate::rules::baseline::{Baseline, BaselineError};
use crate::rules::config::{ArcConfig, ConfigError};
use crate::rules::engine::{CheckRun, check_rules};
use crate::rules::format::{format_status, format_violations, plural};
use crate::ui;
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
    /// Serve the diagram over HTTP and resolve jump targets for an editor
    /// plugin; `--output` has no effect.
    Ui(UiArgs),
}

#[derive(Parser)]
#[command(override_usage = "cargo arc [SHARED OPTIONS] ui [OPTIONS]")]
pub struct UiArgs {
    /// Port to serve on (default: a free port chosen by the OS). A restarted
    /// service passes the port it had, so an open page only needs a reload.
    #[arg(long)]
    pub port: Option<u16>,
}

#[derive(Parser)]
// The shared flags sit on `arc`, so clap's own usage line for a misplaced one
// would send the reader looking for it under `check`.
#[command(override_usage = "cargo arc [SHARED OPTIONS] check [OPTIONS]")]
pub struct CheckArgs {
    /// Path to rules file (default: arc-rules.toml next to the manifest)
    #[arg(long)]
    pub rules: Option<PathBuf>,

    /// List the silenced violations, instead of only counting them
    #[arg(long)]
    pub show_silenced: bool,

    /// Rewrite `arc-baseline.toml` instead of checking
    #[arg(long)]
    pub generate_baseline: bool,
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

/// A run that errored never judged at all, so it arrives as `Err` instead and
/// `main` maps that to exit 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judgment {
    Clean,
    Negative,
}

impl Judgment {
    #[must_use]
    pub fn exit_code(self) -> ExitCode {
        match self {
            Self::Clean => ExitCode::SUCCESS,
            Self::Negative => ExitCode::from(1),
        }
    }
}

#[allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
pub fn run(args: ArcCommand) -> Result<Judgment> {
    if args.common.debug {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::from_default_env().add_directive("cargo_arc=debug".parse().unwrap()),
            )
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }

    if let Some(Command::Check(check_args)) = args.command {
        return run_check(&check_args, &args.common);
    }

    if let Some(Command::Ui(ref ui_args)) = args.command {
        return run_ui(&args, ui_args);
    }

    let vol_config = VolatilityConfig {
        months: args.volatility_months,
        low_threshold: args.volatility_low,
        high_threshold: args.volatility_high,
    };

    if args.volatility {
        run_volatility_report(&args.common.manifest_path, vol_config, args.output.as_ref())?;
        return Ok(Judgment::Clean);
    }

    let analysis = analyze_for_diagram(&args)?;

    let config = RenderConfig {
        expand_level: args.expand_level,
        ..RenderConfig::default()
    };
    let svg = render(&analysis.layout, &config);
    tracing::debug!("phase: render done ({} bytes)", svg.len());
    write_output(&svg, args.output.as_ref())?;
    // The diagram judges nothing, so it can only ever be clean or an error.
    Ok(Judgment::Clean)
}

/// A workspace laid out for a diagram: the layout itself, the jump table
/// [`JumpTable`] assigned while building it, and the workspace root the
/// analysis ran against (`None` when the workspace has no crates).
struct DiagramAnalysis {
    layout: LayoutIR,
    jump_table: JumpTable,
    workspace_root: Option<PathBuf>,
}

/// Analyze the workspace behind `args`: build the dependency graph, lay it
/// out, and enrich it with volatility. Rendering is the caller's job: the
/// callers (`run`, `run_ui`) build their own [`RenderConfig`] and render
/// `analysis.layout` with it.
fn analyze_for_diagram(args: &ArcCommand) -> Result<DiagramAnalysis> {
    let feature_config = build_feature_config(&args.common);

    #[cfg(feature = "hir")]
    let use_hir = args.hir;
    #[cfg(not(feature = "hir"))]
    let use_hir = false;

    let (graph, workspace_root) = build_dependency_graph(
        &args.common.manifest_path,
        &feature_config,
        use_hir,
        args.externals,
        args.transitive_deps,
    )?;
    let reexports = Reexports::from(args.common.include_reexports);
    tracing::debug!("phase: cycle detection start");
    let analysis = graph.production_subgraph(reexports).representative_cycles();
    tracing::debug!(
        "phase: cycle detection done ({} cycles)",
        analysis.cycles.len()
    );
    let (mut layout, jump_table) =
        build_layout(&graph, &analysis, reexports, workspace_root.as_deref());
    tracing::debug!("phase: layout built ({} items)", layout.items.len());

    if !args.no_volatility {
        let vol_config = VolatilityConfig {
            months: args.volatility_months,
            low_threshold: args.volatility_low,
            high_threshold: args.volatility_high,
        };
        enrich_volatility(&mut layout, &args.common.manifest_path, vol_config);
    }

    Ok(DiagramAnalysis {
        layout,
        jump_table,
        workspace_root,
    })
}

/// Run the `ui` subcommand: analyze once with jump ids, then serve the page
/// and resolve jump ids until the process ends.
fn run_ui(args: &ArcCommand, ui_args: &UiArgs) -> Result<Judgment> {
    let analysis = analyze_for_diagram(args)?;
    let config = RenderConfig {
        expand_level: args.expand_level,
        with_jump_ids: true,
        ..RenderConfig::default()
    };
    let svg = render(&analysis.layout, &config);
    let workspace_root = analysis
        .workspace_root
        .context("workspace has no crates to determine its root")?;
    let service = ui::JumpService::new(svg, analysis.jump_table, workspace_root);
    ui::serve(&service, ui_args.port, &mut io::stdout().lock())?;
    Ok(Judgment::Clean)
}

/// Run the `check` subcommand: load rules, evaluate against graph, report violations.
fn run_check(check_args: &CheckArgs, common: &CommonArgs) -> Result<Judgment> {
    let feature_config = build_feature_config(common);

    #[cfg(feature = "hir")]
    let use_hir = false; // check mode doesn't support HIR
    #[cfg(not(feature = "hir"))]
    let use_hir = false;

    let (graph, _workspace_root) = build_dependency_graph(
        &common.manifest_path,
        &feature_config,
        use_hir,
        false,
        false,
    )?;

    let manifest_dir = resolve_repo_path(&common.manifest_path);
    let default_rules_path = manifest_dir.join("arc-rules.toml");
    let rules_path = check_args.rules.as_deref().unwrap_or(&default_rules_path);
    let explicit = check_args.rules.is_some();

    let config = match ArcConfig::load(rules_path) {
        Ok(config) => config,
        // A missing rules file is the default run, not a failure: the implicit
        // cycle rule stands in for it. A file named on the command line is a
        // different matter, its absence is a mistake.
        Err(ConfigError::FileNotFound(..)) if !explicit => ArcConfig::implicit(),
        Err(e) => return Err(e.into()),
    };

    let baseline_path = resolve_repo_path(rules_path).join("arc-baseline.toml");

    if check_args.generate_baseline {
        run_generate_baseline(
            &graph,
            &config,
            rules_path,
            &baseline_path,
            common.include_reexports,
        )?;
        return Ok(Judgment::Clean);
    }

    let baseline = Baseline::load(&baseline_path)?;

    tracing::debug!("phase: rule check start");
    let result = check_rules(&graph, &config, &baseline, common.include_reexports)
        .map_err(|overlap| overlap.in_file(rules_path))?;
    tracing::debug!(
        "phase: rule check done ({} violations)",
        result.reported().count()
    );
    eprint!("{}", format_violations(&result, check_args.show_silenced));
    print!("{}", format_status(&result));

    Ok(if result.has_negative_judgment() {
        Judgment::Negative
    } else {
        Judgment::Clean
    })
}

/// `--generate-baseline`: refuse to write when an `except` pattern matches no
/// module (it would silently freeze violations that pattern should instead be
/// allowing), otherwise rewrite `baseline_path` from the current violations,
/// carrying over the entries of a rule at `Severity::Ignore` as they are.
fn run_generate_baseline(
    graph: &ArcGraph,
    config: &ArcConfig,
    rules_path: &Path,
    baseline_path: &Path,
    include_reexports: bool,
) -> Result<()> {
    let baseline = Baseline::empty();
    let run = CheckRun::new(graph, &baseline, include_reexports);
    let dead = run.dead_excepts(config);
    if !dead.is_empty() {
        let mut message = String::from("cannot write a baseline while an except matches nothing");
        for d in &dead {
            let _ = write!(
                message,
                "\n  rule {:?}: pattern {:?} matches no module",
                d.rule, d.pattern
            );
        }
        message.push_str("\n  fix the pattern or delete the entry, then run again");
        anyhow::bail!(message);
    }

    let mut result = run
        .check_all(config)
        .map_err(|overlap| overlap.in_file(rules_path))?;

    let previous = match Baseline::load(baseline_path) {
        Ok(baseline) => baseline,
        Err(
            err @ (BaselineError::Parse(..)
            | BaselineError::Io(..)
            | BaselineError::UnsupportedVersion(..)),
        ) => {
            eprintln!("cannot read the existing baseline, regenerating without it: {err}");
            Baseline::empty()
        }
        Err(err) => return Err(err.into()),
    };
    result
        .baseline_entries
        .extend(previous.entries_under(config.ignored_rules()));

    let written = Baseline::write(baseline_path, &result.baseline_entries)?;
    eprintln!(
        "wrote {} to {}",
        plural(written, "violation"),
        baseline_path.display()
    );
    Ok(())
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

/// Reads the workspace root off `crates`, all of whose members share it.
/// `None` when the workspace has no crates to determine it from.
fn workspace_root_of(crates: &[CrateInfo]) -> Option<PathBuf> {
    crates.first().map(|krate| krate.workspace_root.clone())
}

fn build_dependency_graph(
    manifest_path: &Path,
    feature_config: &FeatureConfig,
    use_hir: bool,
    externals: bool,
    transitive_deps: bool,
) -> Result<(ArcGraph, Option<PathBuf>)> {
    let crates = analyze_workspace(manifest_path, feature_config)?;
    tracing::debug!("phase: workspace analyzed ({} crates)", crates.len());
    let workspace_root = workspace_root_of(&crates);
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
    Ok((graph, workspace_root))
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

    /// Helper to parse `ArcCommand` via Cargo wrapper
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
    fn test_parse_generate_baseline_flag() {
        let cmd = parse_args(&["cargo", "arc", "check", "--generate-baseline"]);
        match cmd.command {
            Some(Command::Check(ref args)) => assert!(args.generate_baseline),
            _ => panic!("expected Command::Check"),
        }
    }

    #[test]
    fn test_parse_generate_baseline_flag_default() {
        let cmd = parse_args(&["cargo", "arc", "check"]);
        match cmd.command {
            Some(Command::Check(ref args)) => assert!(!args.generate_baseline),
            _ => panic!("expected Command::Check"),
        }
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
    fn test_parse_diagram_default() {
        let cmd = parse_args(&["cargo", "arc"]);
        assert!(cmd.command.is_none());
    }

    #[test]
    fn test_parse_ui_subcommand() {
        let cmd = parse_args(&["cargo", "arc", "ui"]);
        assert!(matches!(
            cmd.command,
            Some(Command::Ui(UiArgs { port: None }))
        ));
    }

    #[test]
    fn test_parse_ui_subcommand_after_common_args() {
        let cmd = parse_args(&["cargo", "arc", "--manifest-path", "x", "ui"]);
        assert!(matches!(cmd.command, Some(Command::Ui(..))));
    }

    #[test]
    fn test_parse_ui_port() {
        let cmd = parse_args(&["cargo", "arc", "ui", "--port", "4321"]);
        assert!(matches!(
            cmd.command,
            Some(Command::Ui(UiArgs { port: Some(4321) }))
        ));
    }

    #[test]
    fn test_parse_common_args_on_check() {
        // Common args must come before the subcommand
        let cmd = parse_args(&["cargo", "arc", "--features", "web", "check"]);
        assert!(matches!(cmd.command, Some(Command::Check(..))));
        assert_eq!(cmd.common.features, vec!["web"]);
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
    #[ignore = "smoke test, requires rust-analyzer (~30s)"]
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

    // ===== workspace root beside the graph =====

    #[test]
    fn build_dependency_graph_returns_the_workspace_root() {
        let manifest =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");
        let (_graph, workspace_root) =
            build_dependency_graph(&manifest, &FeatureConfig::default(), false, false, false)
                .unwrap();
        assert_eq!(
            workspace_root,
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate"))
        );
    }

    #[test]
    fn workspace_root_is_none_for_a_workspace_without_crates() {
        assert_eq!(workspace_root_of(&[]), None);
    }

    #[test]
    fn workspace_root_of_reads_the_first_crates_root() {
        use crate::model::{CrateInfo, TargetRoots};

        let krate = CrateInfo {
            name: "a".into(),
            path: PathBuf::from("/ws/a"),
            workspace_root: PathBuf::from("/ws"),
            target_roots: TargetRoots::default(),
            manifest: PathBuf::from("/ws/a/Cargo.toml"),
            dependencies: vec![],
            dev_dependencies: vec![],
        };
        assert_eq!(workspace_root_of(&[krate]), Some(PathBuf::from("/ws")));
    }

    // ===== analyze_for_diagram =====

    #[test]
    fn analyze_for_diagram_returns_the_jump_table_and_root() {
        use crate::layout::LocationId;

        let manifest =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");
        let cmd = parse_args(&[
            "cargo",
            "arc",
            "--manifest-path",
            manifest.to_str().unwrap(),
        ]);

        let analysis = analyze_for_diagram(&cmd).unwrap();

        assert!(
            analysis.jump_table.resolve(LocationId::from(0)).is_some(),
            "a workspace with crates assigns at least one jump target"
        );
        assert_eq!(
            analysis.workspace_root,
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate"))
        );

        let default_svg = render(&analysis.layout, &RenderConfig::default());
        assert!(!default_svg.contains("\"jump\""));

        let jump_config = RenderConfig {
            with_jump_ids: true,
            ..RenderConfig::default()
        };
        let jump_svg = render(&analysis.layout, &jump_config);
        assert!(jump_svg.contains("\"jump\""));
    }

    /// A function called through its module (`use crate::beta;` then
    /// `beta::helper()`) is a symbol of the edge like an imported item, and
    /// its definition line reaches the layout.
    #[test]
    fn analyze_for_diagram_locates_the_definition_of_a_called_function() {
        let manifest =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");
        let cmd = parse_args(&[
            "cargo",
            "arc",
            "--manifest-path",
            manifest.to_str().unwrap(),
        ]);

        let analysis = analyze_for_diagram(&cmd).unwrap();

        let beta = analysis
            .layout
            .items
            .iter()
            .find(|item| item.label == "beta")
            .expect("beta module item")
            .id;
        let helper = analysis
            .layout
            .symbol_definitions
            .get(&beta)
            .and_then(|symbols| symbols.get("helper"))
            .unwrap_or_else(|| {
                panic!(
                    "beta::helper has a definition, got {:?}",
                    analysis.layout.symbol_definitions
                )
            });
        assert_eq!(helper.line, 3);
    }
}
