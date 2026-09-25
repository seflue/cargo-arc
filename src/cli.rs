use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::{BTreeSet, HashMap};
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
use crate::hotspots;
use crate::layout::{JumpTable, LayoutIR, LocationId, build_layout};
use crate::model::{CrateExportMap, CrateInfo, ModulePathMap, WorkspaceCrates};
use crate::render::{
    AnalysisSwitches, Appearance, RenderConfig, THEMES, Theme, html_page, project_name, render,
    render_hotspots,
};
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

    /// Pin the diagram to a theme by name; `light` and `dark` name the
    /// default theme of that mode. Without it the diagram follows the
    /// system setting, or the editor under `ui`.
    #[arg(long, value_name = "NAME", value_parser = parse_theme)]
    pub theme: Option<&'static Theme>,

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
    /// Render the circle-packed hotspot map (lines × commits) to `-o` or stdout
    Hotspots(HotspotsArgs),
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

#[derive(Parser)]
#[command(override_usage = "cargo arc [SHARED OPTIONS] hotspots [OPTIONS]")]
pub struct HotspotsArgs {
    /// Output file (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Number of top hotspots to rank
    #[arg(long, default_value_t = DEFAULT_HOTSPOTS)]
    pub hotspots: usize,
}

/// The default of `--hotspots`, and the count `ui` ranks its map with.
const DEFAULT_HOTSPOTS: usize = 10;

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

/// The `--theme` value parser: a theme name or a mode alias, or an error
/// that lists what would have been accepted.
fn parse_theme(name: &str) -> Result<&'static Theme, String> {
    Theme::named(name).ok_or_else(|| {
        let names: Vec<&str> = THEMES.iter().map(|theme| theme.name).collect();
        format!(
            "unknown theme '{name}'; known themes: {}, or light / dark for a mode's default",
            names.join(", ")
        )
    })
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

    if let Some(Command::Hotspots(ref hotspots_args)) = args.command {
        return run_hotspots(&args, hotspots_args);
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

    let analysis = analyze_for_diagram(&args, args.switches(), !args.no_volatility)?;

    let config = RenderConfig {
        expand_level: args.expand_level,
        theme: args.theme,
        ..RenderConfig::default()
    };
    let svg = render(&analysis.layout, &config);
    tracing::debug!("phase: render done ({} bytes)", svg.len());
    let document = diagram_document(
        svg,
        args.output.as_deref(),
        analysis.workspace_root.as_deref(),
        args.theme,
    );
    write_output(&document, args.output.as_ref())?;
    // The diagram judges nothing, so it can only ever be clean or an error.
    Ok(Judgment::Clean)
}

/// A workspace laid out for a diagram: the dependency graph, the layout
/// built from it, the jump table [`JumpTable`] assigned while building it,
/// and the workspace root the analysis ran against (`None` when the
/// workspace has no crates). The graph is carried alongside the layout so a
/// second page (the hotspot map) can be built from the same analysis run.
struct DiagramAnalysis {
    graph: ArcGraph,
    layout: LayoutIR,
    jump_table: JumpTable,
    workspace_root: Option<PathBuf>,
}

impl ArcCommand {
    /// The analysis switches as the command line set them.
    fn switches(&self) -> AnalysisSwitches {
        AnalysisSwitches {
            externals: self.externals,
            tests: self.common.include_tests,
        }
    }
}

/// Analyze the workspace behind `args` with `switches` in place of the
/// flags they stand for: build the dependency graph, lay it out, and, when
/// `with_volatility` is set, enrich it with volatility. `run_hotspots`
/// passes `false`: it builds its own separate [`VolatilityAnalyzer`] for the
/// map, so the arc layout's own enrichment would only be discarded work.
/// Rendering is the caller's job: the callers (`run`, `run_ui`) build their
/// own [`RenderConfig`] and render `analysis.layout` with it.
fn analyze_for_diagram(
    args: &ArcCommand,
    switches: AnalysisSwitches,
    with_volatility: bool,
) -> Result<DiagramAnalysis> {
    let feature_config = FeatureConfig {
        include_tests: switches.tests,
        ..build_feature_config(&args.common)
    };

    #[cfg(feature = "hir")]
    let use_hir = args.hir;
    #[cfg(not(feature = "hir"))]
    let use_hir = false;

    let (graph, workspace_root) = build_dependency_graph(
        &args.common.manifest_path,
        &feature_config,
        use_hir,
        switches.externals,
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

    if with_volatility {
        let vol_config = VolatilityConfig {
            months: args.volatility_months,
            low_threshold: args.volatility_low,
            high_threshold: args.volatility_high,
        };
        enrich_volatility(&mut layout, &args.common.manifest_path, vol_config);
    }

    Ok(DiagramAnalysis {
        graph,
        layout,
        jump_table,
        workspace_root,
    })
}

/// Run the `ui` subcommand: analyze with jump ids, then serve both pages and
/// resolve jump ids until the process ends. The service gets both pages as a
/// closure over `args`, so it can run them again with other switches.
/// `--volatility-months` and `--no-volatility` govern the map's volatility
/// here too, same as the arc diagram: `ui` takes no separate hotspots flags.
fn run_ui(args: &ArcCommand, ui_args: &UiArgs) -> Result<Judgment> {
    let pages = |switches: AnalysisSwitches| -> Result<(PathBuf, ui::Pages)> {
        let analysis = analyze_for_diagram(args, switches, !args.no_volatility)?;
        let workspace_root = analysis
            .workspace_root
            .clone()
            .context("workspace has no crates to determine its root")?;

        let arc_config = RenderConfig {
            expand_level: args.expand_level,
            with_jump_ids: true,
            theme: args.theme,
            switches,
            ..RenderConfig::default()
        };
        let arc_svg = render(&analysis.layout, &arc_config);

        let (tree, volatility) =
            build_hotspot_tree(args, &analysis.graph, &workspace_root, DEFAULT_HOTSPOTS);
        let packed = hotspots::pack(&tree);

        let mut table = analysis.jump_table;
        let jump_targets = register_hotspot_targets(&mut table, &tree, &workspace_root);

        let hotspots_config = RenderConfig {
            with_jump_ids: true,
            theme: args.theme,
            volatility,
            ..RenderConfig::default()
        };
        let hotspots_svg = render_hotspots(&tree, &packed, &jump_targets, &hotspots_config);

        Ok((
            workspace_root,
            ui::Pages {
                arc: ui::Diagram { svg: arc_svg },
                hotspots: ui::Diagram { svg: hotspots_svg },
                table,
            },
        ))
    };
    let switches = args.switches();
    let (workspace_root, initial) = pages(switches)?;
    let service = ui::JumpService::new(
        initial,
        switches,
        workspace_root,
        args.theme,
        Box::new(move |switches| pages(switches).map(|(_, pages)| pages)),
    );
    ui::serve(
        &service,
        ui_args.port,
        io::BufReader::new(io::stdin()),
        &mut io::stdout(),
    )?;
    Ok(Judgment::Clean)
}

/// Build the hotspot tree over `graph` with lines from `hotspots::code_lines`
/// and commits from the repository holding `--manifest-path`, over the
/// shared `--volatility-months` window. The map is grey on `--no-volatility`,
/// without usable git, or with no commit in the window.
fn build_hotspot_tree(
    args: &ArcCommand,
    graph: &ArcGraph,
    workspace_root: &Path,
    hotspots_n: usize,
) -> (hotspots::HotspotTree, hotspots::MapVolatility) {
    let months = args.volatility_months;
    let volatility = if args.no_volatility {
        Err(hotspots::GreyCause::Flag)
    } else {
        let mut analyzer = VolatilityAnalyzer::new(VolatilityConfig {
            months,
            ..VolatilityConfig::default()
        });
        match analyzer.analyze(resolve_repo_path(&args.common.manifest_path)) {
            Ok(()) => Ok(analyzer),
            Err(err) => {
                tracing::warn!("Volatility analysis skipped: {err}");
                Err(hotspots::GreyCause::GitUnavailable)
            }
        }
    };
    let tree = hotspots::build(
        graph,
        workspace_root,
        // An unreadable file becomes a zero-line leaf rather than failing
        // the whole map over one file.
        |file: &Path| hotspots::code_lines(file).unwrap_or(0),
        |file: &Path| {
            volatility.as_ref().map_or_else(
                |_| BTreeSet::new(),
                |analyzer| analyzer.commits(file).clone(),
            )
        },
        hotspots_n,
    );
    let map_volatility = match volatility {
        Err(cause) => hotspots::MapVolatility::Grey(cause),
        Ok(_) if tree.total_commits == 0 => {
            hotspots::MapVolatility::Grey(hotspots::GreyCause::NoCommitsInWindow { months })
        }
        Ok(_) => hotspots::MapVolatility::Window { months },
    };
    (tree, map_volatility)
}

/// Add a line-1 jump target to `table` for every leaf of `tree`, and return
/// each leaf's file mapped to its id. `node_files` keeps the file's arc node.
fn register_hotspot_targets(
    table: &mut JumpTable,
    tree: &hotspots::HotspotTree,
    workspace_root: &Path,
) -> HashMap<PathBuf, LocationId> {
    fn walk(
        node: &hotspots::HotspotNode,
        table: &mut JumpTable,
        workspace_root: &Path,
        targets: &mut HashMap<PathBuf, LocationId>,
    ) {
        if node.kind == hotspots::HotspotKind::File {
            let id = table.insert(workspace_root.join(&node.file), 1);
            targets.insert(node.file.clone(), id);
        }
        for child in &node.children {
            walk(child, table, workspace_root, targets);
        }
    }
    let mut targets = HashMap::new();
    walk(&tree.root, table, workspace_root, &mut targets);
    targets
}

/// Run the `hotspots` subcommand: build the map over the diagram's dependency
/// graph and write it to `-o` or stdout, as the arc diagram's `-o` does.
fn run_hotspots(args: &ArcCommand, hotspots_args: &HotspotsArgs) -> Result<Judgment> {
    let analysis = analyze_for_diagram(args, args.switches(), false)?;
    let workspace_root = analysis
        .workspace_root
        .context("workspace has no crates to build a hotspot map from")?;

    let (tree, volatility) = build_hotspot_tree(
        args,
        &analysis.graph,
        &workspace_root,
        hotspots_args.hotspots,
    );
    let packed = hotspots::pack(&tree);

    let config = RenderConfig {
        theme: args.theme,
        volatility,
        ..RenderConfig::default()
    };
    // The standalone file carries no jump service to resolve an id against
    // (`with_jump_ids` stays false in `config`, as for the plain arc
    // diagram's own `-o` file), so no jump ids are assigned here either.
    let svg = render_hotspots(&tree, &packed, &HashMap::new(), &config);
    let document = diagram_document(
        svg,
        hotspots_args.output.as_deref(),
        Some(&workspace_root),
        args.theme,
    );
    write_output(&document, hotspots_args.output.as_ref())?;
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
    let dead = run.dead_allows(config);
    if !dead.is_empty() {
        let mut message =
            String::from("cannot write a baseline while an allow entry matches nothing");
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

/// The bytes `-o` writes: the SVG itself, or for an `.html` / `.xhtml` name
/// the page `arc ui` serves, so the file opens in a browser tab or a
/// webview at its own size. The page root pins `theme` like the SVG root.
fn diagram_document(
    svg: String,
    output: Option<&Path>,
    workspace_root: Option<&Path>,
    theme: Option<&'static Theme>,
) -> String {
    let wants_html = output
        .and_then(Path::extension)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("xhtml"));
    if wants_html {
        html_page(
            &svg,
            workspace_root.and_then(project_name),
            Appearance { theme, mode: None },
        )
    } else {
        svg
    }
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

    #[test]
    fn html_output_name_wraps_the_svg_in_a_page() {
        let svg = "<svg/>".to_string();
        for name in ["deps.html", "deps.xhtml", "deps.HTML"] {
            let page = diagram_document(svg.clone(), Some(Path::new(name)), None, None);
            assert!(page.contains("<html"), "{name}: {page}");
            assert!(page.contains("<svg/>"), "{name}: {page}");
        }
    }

    #[test]
    fn html_output_titles_the_page_after_the_workspace_root() {
        let page = diagram_document(
            "<svg/>".to_string(),
            Some(Path::new("deps.html")),
            Some(Path::new("/home/u/my-ws")),
            None,
        );
        assert!(page.contains("<title>my-ws · cargo-arc</title>"), "{page}");
    }

    #[test]
    fn other_output_names_and_stdout_keep_the_svg() {
        let svg = "<svg/>".to_string();
        assert_eq!(
            diagram_document(svg.clone(), Some(Path::new("deps.svg")), None, None),
            svg
        );
        assert_eq!(diagram_document(svg.clone(), None, None, None), svg);
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
    fn theme_flag_takes_a_name_or_a_mode_alias() {
        let name = |args: &[&str]| parse_args(args).theme.map(|theme| theme.name);
        assert_eq!(name(&["cargo", "arc", "--theme", "mocha"]), Some("mocha"));
        assert_eq!(name(&["cargo", "arc", "--theme", "dark"]), Some("mocha"));
        assert_eq!(name(&["cargo", "arc", "--theme", "light"]), Some("latte"));
        assert_eq!(name(&["cargo", "arc"]), None);
    }

    #[test]
    fn unknown_theme_is_an_error_listing_the_names() {
        let err = Cargo::try_parse_from(["cargo", "arc", "--theme", "frappe"])
            .err()
            .expect("an unknown theme is rejected")
            .to_string();
        assert!(err.contains("frappe"), "{err}");
        assert!(err.contains("latte") && err.contains("mocha"), "{err}");
        assert!(err.contains("light") && err.contains("dark"), "{err}");
    }

    #[test]
    fn html_output_pins_the_theme_on_the_page_root() {
        let page = diagram_document(
            "<svg/>".to_string(),
            Some(Path::new("deps.html")),
            None,
            Theme::named("mocha"),
        );
        assert!(
            page.contains("<html xmlns=\"http://www.w3.org/1999/xhtml\" data-theme=\"mocha\">"),
            "{page}"
        );
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
            theme: None,
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

        let analysis = analyze_for_diagram(&cmd, cmd.switches(), !cmd.no_volatility).unwrap();

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

    // ===== register_hotspot_targets =====

    /// A hotspot leaf shares its file with the arc module `build_layout`
    /// registered in `node_files`; registering the leaf must keep that entry.
    #[test]
    fn register_hotspot_targets_does_not_overwrite_the_arc_nodes_own_file_lookup() {
        use crate::hotspots::{HotspotKind, HotspotNode, HotspotTree};

        let workspace_root = PathBuf::from("/ws");
        let mut table = JumpTable::new();
        let module_id = table.insert(PathBuf::from("/ws/app/src/hot.rs"), 1);
        table.insert_node_files("42", [module_id]);

        let leaf = HotspotNode {
            name: "hot.rs".to_string(),
            kind: HotspotKind::File,
            file: PathBuf::from("app/src/hot.rs"),
            lines: 10,
            commits: 0,
            children: Vec::new(),
        };
        let tree = HotspotTree {
            root: leaf,
            max_file_commits: 0,
            total_commits: 0,
            hotspots: Vec::new(),
        };

        register_hotspot_targets(&mut table, &tree, &workspace_root);

        assert_eq!(
            table.node_at(Path::new("/ws/app/src/hot.rs")),
            Some("42"),
            "a hotspot leaf sharing a file with an arc module must not overwrite the arc node lookup for it"
        );
    }

    /// A graph and workspace root for `build_hotspot_tree`'s own tests: the
    /// tree's shape does not matter to them, only the cause it returns.
    fn hotspot_tree_fixture() -> (ArcGraph, PathBuf) {
        let manifest =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_crate/Cargo.toml");
        let (graph, workspace_root) =
            build_dependency_graph(&manifest, &FeatureConfig::default(), false, false, false)
                .unwrap();
        (graph, workspace_root.unwrap())
    }

    #[test]
    fn build_hotspot_tree_names_the_flag_as_the_cause_when_no_volatility_is_set() {
        let (graph, workspace_root) = hotspot_tree_fixture();
        let manifest = workspace_root.join("Cargo.toml");
        let cmd = parse_args(&[
            "cargo",
            "arc",
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--no-volatility",
        ]);

        let (_tree, cause) = build_hotspot_tree(&cmd, &graph, &workspace_root, DEFAULT_HOTSPOTS);

        assert_eq!(
            cause,
            hotspots::MapVolatility::Grey(hotspots::GreyCause::Flag)
        );
    }

    /// A manifest whose directory sits outside any git repository (a fresh
    /// tempdir): `VolatilityAnalyzer::analyze` fails, so the cause is that
    /// git could not be run, not the flag or an empty window.
    #[test]
    fn build_hotspot_tree_names_git_unavailable_outside_a_repository() {
        let (graph, workspace_root) = hotspot_tree_fixture();
        let outside = tempfile::tempdir().unwrap();
        let manifest = outside.path().join("Cargo.toml");
        let cmd = parse_args(&[
            "cargo",
            "arc",
            "--manifest-path",
            manifest.to_str().unwrap(),
        ]);

        let (_tree, cause) = build_hotspot_tree(&cmd, &graph, &workspace_root, DEFAULT_HOTSPOTS);

        assert_eq!(
            cause,
            hotspots::MapVolatility::Grey(hotspots::GreyCause::GitUnavailable)
        );
    }

    /// A freshly `git init`ed directory with no commits: `analyze` succeeds,
    /// but the window holds nothing to report, so the cause names that
    /// rather than a flag or an unavailable git.
    #[test]
    fn build_hotspot_tree_names_the_empty_window_when_git_has_no_commits() {
        let (graph, workspace_root) = hotspot_tree_fixture();
        let repo = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(repo.path())
            .status()
            .expect("git should be on PATH");
        assert!(status.success());

        let manifest = repo.path().join("Cargo.toml");
        let cmd = parse_args(&[
            "cargo",
            "arc",
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--volatility-months",
            "3",
        ]);

        let (_tree, cause) = build_hotspot_tree(&cmd, &graph, &workspace_root, DEFAULT_HOTSPOTS);

        assert_eq!(
            cause,
            hotspots::MapVolatility::Grey(hotspots::GreyCause::NoCommitsInWindow { months: 3 })
        );
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

        let analysis = analyze_for_diagram(&cmd, cmd.switches(), !cmd.no_volatility).unwrap();

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
