//! Rule evaluation engine
//!
//! Checks architecture rules against the dependency graph and collects violations.

use crate::diagnose::{Cluster, Cycle, CycleAnalysis, RepresentativeCycles};
use crate::graph::{ArcGraph, EdgeWeight};
use crate::model::{Edge, EdgeSymbols, SourceLocation};
use crate::rules::baseline::{Baseline, BaselineEntry, ViolationKey};
use crate::rules::config::{
    ArcConfig, DiagnosticLevel, Direction, Except, ForbiddenDependencyRule, Layer, LayersRule,
    NoCyclesRule, Rule, RuleKind, Severity,
};
use crate::rules::diagnostics::{self, Diagnostic};
use crate::rules::matching::PatternIndex;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Whether a violation counts, and if not, what silenced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationState {
    Reported,
    Allowed,
    Frozen,
}

/// Two ordinary positions of one `layers` rule claim the same node. No
/// diagnostic level has a defensible reading here: `allow` would leave a
/// judgment about the architecture hanging on array order. The run fails
/// outright instead of reporting a violation.
#[derive(Debug)]
pub struct LayerOverlapError {
    pub rule: String,
    pub node: String,
    pub first: Vec<String>,
    pub second: Vec<String>,
}

impl LayerOverlapError {
    /// Carry the rules file the overlap was written in. The engine resolves
    /// patterns against a graph and never opens that file, so the path is
    /// attached where it is known.
    #[must_use]
    pub fn in_file(self, path: &Path) -> LayerOverlapInFile {
        LayerOverlapInFile {
            path: path.to_path_buf(),
            overlap: self,
        }
    }

    fn fmt_in(&self, f: &mut std::fmt::Formatter<'_>, path: Option<&Path>) -> std::fmt::Result {
        write!(f, "rule {:?}", self.rule)?;
        if let Some(path) = path {
            write!(f, " in {}", path.display())?;
        }
        write!(
            f,
            ": {:?} matches both layer {:?} and layer {:?}; a layers rule \
             gives a node one position, so split it into two rules",
            self.node, self.first, self.second
        )
    }
}

impl std::fmt::Display for LayerOverlapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_in(f, None)
    }
}

impl std::error::Error for LayerOverlapError {}

/// An overlap together with the rules file it stands in. Every `ConfigError`
/// names its file, and this failure is of the same class: the file says
/// something a `layers` rule cannot mean.
#[derive(Debug)]
pub struct LayerOverlapInFile {
    path: PathBuf,
    overlap: LayerOverlapError,
}

impl std::fmt::Display for LayerOverlapInFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.overlap.fmt_in(f, Some(&self.path))
    }
}

impl std::error::Error for LayerOverlapInFile {}

/// A single architecture rule violation.
#[derive(Debug)]
pub struct Violation {
    pub rule_name: String,
    pub rule_type: String,
    pub severity: Severity,
    pub state: ViolationState,
    pub detail: ViolationDetail,
    pub locations: Vec<SourceLocation>,
}

/// Violation payload: structured data the renderer branches on.
#[derive(Debug)]
pub enum ViolationDetail {
    /// One dependency edge. forbidden-dependency, layers and no-cycles all
    /// report this shape.
    Edge {
        edge: Edge,
        /// What a baseline entry tolerates on this edge. `Some` only where that
        /// no longer covers what the edge carries, the one case the report has
        /// to explain.
        frozen_for: Option<EdgeSymbols>,
        /// The edges a dependency runs through to reach `edge.to`, in order.
        /// Empty where the pair is written as an edge of its own.
        via: Vec<WrittenEdge>,
    },
    Cluster(CycleCluster),
}

/// One edge as the code writes it: the pair it connects and the imports that
/// write it, none for a crate dependency.
#[derive(Debug)]
pub struct WrittenEdge {
    pub edge: Edge,
    pub locations: Vec<SourceLocation>,
}

/// One cyclic cluster, resolved to names and counts for rendering.
#[derive(Debug)]
pub struct CycleCluster {
    /// 1-based position among the clusters of the same rule and violation
    /// state; reported and silenced tangles are numbered separately.
    pub position: usize,
    pub total: usize,
    pub crate_name: String,
    /// Common module prefix of all members, or the crate alone when they share
    /// none. A cluster is not a stable object (one new edge can merge two of
    /// them), so a name would promise an identity it doesn't have; the cluster
    /// is never the argument of a command, only a location for one.
    pub place: String,
    pub modules: usize,
    pub cycles: usize,
    /// Crate-relative member names, set when the cluster holds exactly one cycle.
    pub cycle: Option<Vec<String>>,
    /// Feedback edges, crate-relative names, ranked as `Cluster::feedback_edges`.
    pub feedback_edges: Vec<CycleClusterEdge>,
}

/// One feedback edge of a [`CycleCluster`], names already resolved.
#[derive(Debug)]
pub struct CycleClusterEdge {
    pub from: String,
    pub to: String,
    pub cycles: usize,
    pub symbols: usize,
}

impl CycleCluster {
    /// Resolve `cluster` to names and counts. `analysis` must be the one
    /// `cluster` was produced from, so its cycle indices resolve correctly.
    pub(crate) fn from_cluster(
        graph: &ArcGraph,
        analysis: &CycleAnalysis,
        cluster: &Cluster,
        position: usize,
        total: usize,
    ) -> Self {
        let crate_name = graph[cluster.crate_idx].name().to_string();
        let place = common_place(graph, &cluster.nodes);
        let cycle = (cluster.cycles.len() == 1).then(|| {
            analysis.cycles[cluster.cycles[0]]
                .nodes
                .iter()
                .map(|&idx| rel_name(graph, idx, &crate_name))
                .collect()
        });
        let feedback_edges = cluster
            .feedback_edges
            .iter()
            .map(|edge| CycleClusterEdge {
                from: rel_name(graph, edge.from, &crate_name),
                to: rel_name(graph, edge.to, &crate_name),
                cycles: edge.cycles,
                symbols: edge.symbols,
            })
            .collect();
        Self {
            position,
            total,
            crate_name,
            place,
            modules: cluster.nodes.len(),
            cycles: cluster.cycles.len(),
            cycle,
            feedback_edges,
        }
    }
}

/// What the baseline says about the cyclic edges of one `no-cycles` rule.
#[derive(Default)]
struct CyclicEdges {
    /// Edges whose entry covers everything they carry.
    covered: HashSet<(NodeIndex, NodeIndex)>,
    /// Edges with an entry that no longer covers them.
    outgrown: Vec<Violation>,
    entries: Vec<BaselineEntry>,
    hits: Vec<BaselineEntry>,
}

/// One dependency to hold against a rule: its two ends and what carries it.
struct Dependency {
    source: NodeIndex,
    target: NodeIndex,
    carrier: Carrier,
}

/// What carries a dependency from its source to its target.
enum Carrier {
    /// The pair is an edge of the graph. Holds the imports that write it, none
    /// for a crate dependency, which is written in a manifest.
    Written(Vec<SourceLocation>),
    /// The pair is reached over other nodes. Holds the edges from source to
    /// target, in order.
    Path(Vec<WrittenEdge>),
}

impl Carrier {
    /// The symbols crossing the dependency, as the baseline compares and
    /// stores them. A path is frozen by its pair alone and has none.
    fn observed_symbols(&self) -> EdgeSymbols {
        match self {
            Self::Written(locations) => EdgeSymbols::from_locations(locations),
            Self::Path(_) => EdgeSymbols::default(),
        }
    }
}

/// A way from the `source` of [`CheckRun::routes_over_unpositioned`] to a
/// positioned node, as the unpositioned nodes between. `over` is never empty.
struct Route {
    target: NodeIndex,
    over: Vec<NodeIndex>,
}

/// The edges of `route` from `source`, in order, each with the imports that
/// write it.
fn written_edges(graph: &ArcGraph, source: NodeIndex, route: &Route) -> Vec<WrittenEdge> {
    let nodes: Vec<NodeIndex> = std::iter::once(source)
        .chain(route.over.iter().copied())
        .chain(std::iter::once(route.target))
        .collect();
    nodes
        .windows(2)
        .map(|pair| WrittenEdge {
            edge: Edge::new(graph.qualified_name(pair[0]), graph.qualified_name(pair[1])),
            locations: module_dep_locations(graph, pair[0], pair[1]),
        })
        .collect()
}

/// One violation on `edge`. A written carrier fills the violation's
/// `locations`, a path its `via`; the other stays empty.
fn edge_violation(
    rule: &Rule,
    edge: &Edge,
    carrier: Carrier,
    frozen_for: Option<EdgeSymbols>,
    state: ViolationState,
) -> Violation {
    let (locations, via) = match carrier {
        Carrier::Written(locations) => (locations, Vec::new()),
        Carrier::Path(via) => (Vec::new(), via),
    };
    Violation {
        rule_name: rule.name.clone(),
        rule_type: rule.rule_type().into(),
        severity: rule.severity,
        state,
        detail: ViolationDetail::Edge {
            edge: edge.clone(),
            frozen_for,
            via,
        },
        locations,
    }
}

/// Every edge running inside a non-trivial component of `sub`, as index pairs
/// of the graph `sub` was built from. Edges between components carry no cycle.
fn scc_internal_edges(sub: &DiGraph<NodeIndex, ()>) -> Vec<(NodeIndex, NodeIndex)> {
    let component: HashMap<NodeIndex, usize> = tarjan_scc(sub)
        .into_iter()
        .filter(|members| members.len() > 1)
        .enumerate()
        .flat_map(|(id, members)| members.into_iter().map(move |node| (node, id)))
        .collect();
    sub.edge_indices()
        .filter_map(|edge| {
            let (source, target) = sub.edge_endpoints(edge)?;
            (component.get(&source)? == component.get(&target)?)
                .then_some((sub[source], sub[target]))
        })
        .collect()
}

/// The cycle's edges, closing back to its first member.
fn cycle_edges(cycle: &Cycle) -> impl Iterator<Item = (NodeIndex, NodeIndex)> + '_ {
    cycle
        .nodes
        .iter()
        .zip(cycle.nodes.iter().cycle().skip(1))
        .map(|(&from, &to)| (from, to))
}

/// Common `::`-prefix over the crate-qualified names of `nodes`, segment-wise.
/// Module cycles are intra-crate, so at least the crate segment is always
/// shared.
fn common_place(graph: &ArcGraph, nodes: &[NodeIndex]) -> String {
    let mut paths = nodes.iter().map(|&n| {
        graph
            .qualified_name(n)
            .split("::")
            .map(str::to_string)
            .collect::<Vec<_>>()
    });
    let mut prefix = paths.next().unwrap_or_default();
    for path in paths {
        let common = prefix.iter().zip(&path).take_while(|(a, b)| a == b).count();
        prefix.truncate(common);
    }
    prefix.join("::")
}

/// Fully-qualified module name with the leading `<crate_name>::` stripped.
fn rel_name(graph: &ArcGraph, idx: NodeIndex, crate_name: &str) -> String {
    let qualified = graph.qualified_name(idx);
    qualified
        .strip_prefix(&format!("{crate_name}::"))
        .map(String::from)
        .unwrap_or(qualified)
}

/// Aggregated result of checking all rules.
#[derive(Debug, Default)]
pub struct CheckResult {
    /// Per rule, the reported violations first, then the allowed, then the
    /// frozen: `format_violations` groups this back into one block per rule.
    pub violations: Vec<Violation>,
    /// The key of every violation actually reported, i.e. what
    /// `--generate-baseline` writes out.
    pub baseline_entries: Vec<BaselineEntry>,
    /// Every violation the baseline has an entry for, carrying the symbols this
    /// run observed rather than the frozen ones. An edge that outgrew its entry
    /// is in here too: the entry matched, it only stopped covering.
    pub baseline_hits: Vec<BaselineEntry>,
    /// Gaps in the configuration, unrelated to any single rule.
    pub diagnostics: Vec<Diagnostic>,
    /// The rules this run checked, in the order they are written. A rule that
    /// found nothing appears here and nowhere else.
    pub checked_rules: Vec<String>,
}

impl CheckResult {
    /// Violations that count: neither allowed nor frozen.
    pub fn reported(&self) -> impl Iterator<Item = &Violation> {
        self.violations
            .iter()
            .filter(|v| v.state == ViolationState::Reported)
    }

    /// Violations permitted by an `except` entry: they never affect
    /// `has_errors`/`exit_code`, and are only printed under
    /// `--show-silenced`.
    pub fn allowed(&self) -> impl Iterator<Item = &Violation> {
        self.violations
            .iter()
            .filter(|v| v.state == ViolationState::Allowed)
    }

    /// Violations an `arc-baseline.toml` entry covers: like `allowed`, they
    /// never affect `has_errors`/`exit_code`.
    pub fn frozen(&self) -> impl Iterator<Item = &Violation> {
        self.violations
            .iter()
            .filter(|v| v.state == ViolationState::Frozen)
    }

    /// Whether any reported violation has `Severity::Error`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.reported().any(|v| v.severity == Severity::Error)
    }

    #[must_use]
    pub fn has_negative_judgment(&self) -> bool {
        let denied = self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Deny);
        self.has_errors() || denied
    }
}

impl FromIterator<CheckResult> for CheckResult {
    fn from_iter<I: IntoIterator<Item = CheckResult>>(iter: I) -> Self {
        iter.into_iter().fold(Self::default(), |mut acc, result| {
            acc.violations.extend(result.violations);
            acc.baseline_entries.extend(result.baseline_entries);
            acc.baseline_hits.extend(result.baseline_hits);
            acc.diagnostics.extend(result.diagnostics);
            acc
        })
    }
}

/// One check of one graph. Rules and configuration diagnostics share the index
/// built here rather than each building their own.
pub(crate) struct CheckRun<'graph> {
    pattern_index: PatternIndex<'graph>,
    baseline: &'graph Baseline,
    include_reexports: bool,
}

impl<'graph> CheckRun<'graph> {
    pub(crate) fn new(
        graph: &'graph ArcGraph,
        baseline: &'graph Baseline,
        include_reexports: bool,
    ) -> Self {
        Self {
            pattern_index: PatternIndex::build(graph),
            baseline,
            include_reexports,
        }
    }

    fn graph(&self) -> &'graph ArcGraph {
        self.pattern_index.graph()
    }

    fn resolve(&self, pattern: &str) -> Vec<NodeIndex> {
        self.pattern_index.resolve(pattern)
    }

    fn resolve_set(&self, pattern: &str) -> HashSet<NodeIndex> {
        self.resolve(pattern).into_iter().collect()
    }

    /// Dispatch one rule to the checker for its kind, ignoring its severity.
    ///
    /// # Errors
    /// `LayerOverlapError` if a `layers` rule's ordinary positions both claim
    /// one node.
    fn check_rule(&self, rule: &Rule) -> Result<CheckResult, LayerOverlapError> {
        match &rule.kind {
            RuleKind::ForbiddenDependency(params) => Ok(self.check_forbidden(rule, params)),
            RuleKind::NoCycles(params) => Ok(self.check_cycles(rule, params)),
            RuleKind::Layers(params) => self.check_layers(rule, params),
        }
    }

    /// Check a `forbidden-dependency` rule: any production edge from `from`
    /// nodes to `to` nodes is a violation.
    fn check_forbidden(&self, rule: &Rule, params: &ForbiddenDependencyRule) -> CheckResult {
        let from_set = self.resolve_set(&params.from);
        let to_set = self.resolve_set(&params.to);
        let except = ResolvedExceptions::resolve(&rule.except, self);
        let is_violation = |source: NodeIndex, target: NodeIndex| {
            from_set.contains(&source) && to_set.contains(&target)
        };

        let dependencies = self.written_dependencies(&is_violation);
        self.check_dependency_violations(rule, &except, dependencies)
    }

    /// Check a `no-cycles` rule: find the cycles within the scoped
    /// subgraph. Pure re-export cycles are excluded unless `include_reexports`
    /// is set (ADR-022). An edge covered by `except` is removed before the
    /// search, so a cycle built through it never forms; if it lay on one, it is
    /// reported as allowed instead. The allowed side holds removed
    /// edges, not cycles: the two sides can have different SCC decompositions,
    /// so there is no shared cluster to report against.
    fn check_cycles(&self, rule: &Rule, params: &NoCyclesRule) -> CheckResult {
        let graph = self.graph();
        let scope_set = self.resolve_set(&params.scope);
        let except = ResolvedExceptions::resolve(&rule.except, self);
        let mut excepted: Vec<(NodeIndex, NodeIndex)> = Vec::new();

        // Build a subgraph with only production module-dep edges between scope nodes.
        // Pure re-export edges are excluded by default (ADR-022): idiomatic
        // republishing is not a real cycle unless --include-reexports asks for it.
        // Excepted edges are still in at this point, so the components below see
        // the graph each of them actually sits in.
        let mut subgraph = graph.filter_map(
            |idx, _| scope_set.contains(&idx).then_some(idx),
            |edge_idx, edge| {
                if !edge.is_production_module_dep()
                    || (!self.include_reexports && edge.is_reexport_module_dep())
                {
                    return None;
                }
                if !except.is_empty() {
                    let (source, target) =
                        graph.edge_endpoints(edge_idx).expect("edge should exist");
                    if except.covers(source, target) {
                        excepted.push((source, target));
                    }
                }
                Some(())
            },
        );

        let allowed: Vec<Violation> = drop_excepted_edges(&mut subgraph, excepted)
            .into_iter()
            .map(|(source, target)| {
                let edge = Edge::new(graph.qualified_name(source), graph.qualified_name(target));
                let locations = module_dep_locations(graph, source, target);
                edge_violation(
                    rule,
                    &edge,
                    Carrier::Written(locations),
                    None,
                    ViolationState::Allowed,
                )
            })
            .collect();

        let mut cyclic = self.freeze_cycle_edges(rule, &subgraph);

        // A cycle is frozen when every one of its edges is. Every SCC-internal
        // edge lies on at least one representative cycle, so that verdict
        // reaches every cycle. One predicate for both readers below, so the run
        // and its cluster report cannot disagree.
        let tolerated =
            |cycle: &Cycle| cycle_edges(cycle).all(|edge| cyclic.covered.contains(&edge));

        // The cluster report is computed per rule, over that rule's own scoped
        // subgraph: two no-cycles rules with different scopes see different views.
        // `tolerated` decides per cluster whether it carries an untolerated cycle
        // of its own (`Cluster::tolerated == false`, reported) or is wholly frozen
        // (reported as one frozen tangle instead of its individual edges).
        let analysis = subgraph.representative_cycles();
        let report = graph.cluster_report(&subgraph, &analysis, tolerated);

        // Position and total are counted within each marking group: without
        // `--show-silenced` only the reported group prints, so its numbering
        // must not skip over frozen tangles the reader never sees.
        let (frozen_clusters, reported_clusters): (Vec<_>, Vec<_>) = report
            .clusters
            .iter()
            .partition(|cluster| cluster.tolerated);

        let cluster_violation =
            |state: ViolationState, i: usize, total: usize, cluster: &Cluster| Violation {
                rule_name: rule.name.clone(),
                rule_type: rule.rule_type().into(),
                severity: rule.severity,
                state,
                detail: ViolationDetail::Cluster(CycleCluster::from_cluster(
                    graph,
                    &analysis,
                    cluster,
                    i + 1,
                    total,
                )),
                locations: Vec::new(),
            };

        let reported_total = reported_clusters.len();
        let mut reported: Vec<Violation> = reported_clusters
            .into_iter()
            .enumerate()
            .map(|(i, cluster)| {
                cluster_violation(ViolationState::Reported, i, reported_total, cluster)
            })
            .collect();
        reported.append(&mut cyclic.outgrown);

        let frozen_total = frozen_clusters.len();
        let frozen: Vec<Violation> = frozen_clusters
            .into_iter()
            .enumerate()
            .map(|(i, cluster)| cluster_violation(ViolationState::Frozen, i, frozen_total, cluster))
            .collect();

        let mut violations = reported;
        violations.extend(allowed);
        violations.extend(frozen);

        CheckResult {
            violations,
            baseline_entries: cyclic.entries,
            baseline_hits: cyclic.hits,
            diagnostics: Vec::new(),
            checked_rules: Vec::new(),
        }
    }

    /// Hold every edge inside a component of `subgraph` against the baseline.
    ///
    /// Keyed per edge rather than per cluster: a cluster merges and splits as
    /// edges are added, so it has no identity to freeze against.
    fn freeze_cycle_edges(&self, rule: &Rule, subgraph: &DiGraph<NodeIndex, ()>) -> CyclicEdges {
        let graph = self.graph();
        let mut found = CyclicEdges::default();
        for (source, target) in scc_internal_edges(subgraph) {
            let edge = Edge::new(graph.qualified_name(source), graph.qualified_name(target));
            let locations = module_dep_locations(graph, source, target);
            let symbols = EdgeSymbols::from_locations(&locations);
            let tolerated = self.baseline.frozen_for(&rule.name, &edge);
            let covered = tolerated.is_some_and(|t| t.covers(&symbols));
            let entry = BaselineEntry {
                rule: rule.name.clone(),
                key: ViolationKey {
                    edge: edge.clone(),
                    symbols,
                },
            };
            if tolerated.is_some() {
                found.hits.push(entry.clone());
            }
            if covered {
                found.covered.insert((source, target));
            } else {
                found.entries.push(entry);
                // An edge that outgrew its entry is reported beside its
                // cluster, not inside it: the cluster block says which edges
                // carry the tangle, this says why one of them turned red.
                if let Some(tolerated) = tolerated {
                    found.outgrown.push(edge_violation(
                        rule,
                        &edge,
                        Carrier::Written(locations),
                        Some(tolerated.clone()),
                        ViolationState::Reported,
                    ));
                }
            }
        }
        found
    }

    /// Check a `layers` rule: edges must respect layer ordering. A node
    /// claimed by two ordinary positions fails the run rather than silently
    /// landing in whichever position resolved it last.
    ///
    /// # Errors
    /// `LayerOverlapError` if two ordinary positions both claim one node.
    fn check_layers(
        &self,
        rule: &Rule,
        params: &LayersRule,
    ) -> Result<CheckResult, LayerOverlapError> {
        // Build layer index: NodeIndex → layer position
        let mut layer_index: HashMap<NodeIndex, usize> = HashMap::new();
        // Every overlap found, not just the first: which node a `HashSet`
        // resolves first is unstable between runs, so picking one as soon as
        // it turns up would report a different node each time for the same
        // input. Collecting them all first and then choosing by qualified
        // name below is what makes the report reproducible.
        let mut overlaps: Vec<(NodeIndex, usize, usize)> = Vec::new();
        for (pos, layer) in params.layers.iter().enumerate() {
            let Some(patterns) = layer.patterns() else {
                continue; // the catch-all is assigned below, once every ordinary claim is known
            };
            for idx in patterns.iter().flat_map(|pattern| self.resolve(pattern)) {
                if let Some(&earlier_pos) = layer_index.get(&idx) {
                    if earlier_pos != pos {
                        overlaps.push((idx, earlier_pos, pos));
                    }
                    continue;
                }
                layer_index.insert(idx, pos);
            }
        }
        // A crate's qualified name is a prefix of its modules', so it sorts
        // first and names the place the reader has to edit, rather than an
        // arbitrary module beneath it.
        if let Some(&(node, first_pos, second_pos)) = overlaps
            .iter()
            .min_by_key(|(idx, ..)| self.graph().qualified_name(*idx))
        {
            let position_patterns = |pos: usize| {
                params.layers[pos]
                    .patterns()
                    .expect("only ordinary positions can overlap")
                    .to_vec()
            };
            return Err(LayerOverlapError {
                rule: rule.name.clone(),
                node: self.graph().qualified_name(node),
                first: position_patterns(first_pos),
                second: position_patterns(second_pos),
            });
        }
        if let Some(rest) = self.pattern_index.layer_rest(&params.layers) {
            let pos = params
                .layers
                .iter()
                .position(Layer::is_catch_all)
                .expect("layer_rest returned Some only when a catch-all position exists");
            for idx in rest {
                layer_index.insert(idx, pos);
            }
        }

        let except = ResolvedExceptions::resolve(&rule.except, self);

        let is_violation = |source: NodeIndex, target: NodeIndex| {
            let (Some(&source_layer), Some(&target_layer)) =
                (layer_index.get(&source), layer_index.get(&target))
            else {
                return false;
            };
            match params.direction {
                // top-down: higher layers (lower index) may depend on lower layers (higher index)
                Direction::TopDown => source_layer > target_layer,
                // bottom-up: lower layers may depend on higher layers
                Direction::BottomUp => source_layer < target_layer,
            }
        };

        // The order forbids the pair, so a dependency that reaches its target
        // over nodes the rule left unpositioned counts like a written edge.
        let positioned: HashSet<NodeIndex> = layer_index.keys().copied().collect();
        let mut dependencies = self.written_dependencies(&is_violation);
        dependencies.extend(self.dependencies_over_unpositioned(&positioned, &is_violation));
        Ok(self.check_dependency_violations(rule, &except, dependencies))
    }

    /// Dependencies from one positioned node to another that run over one or
    /// more unpositioned nodes, limited to those `is_violation` rejects.
    ///
    /// A route stops at the first positioned node `p` it meets. The order is
    /// transitive, so where `a -> p` and `p -> b` are both allowed, `a -> b`
    /// is allowed too, and both halves are held against the rule on their own.
    ///
    /// The stop also keeps a silenced edge from producing findings elsewhere.
    /// `except` and the baseline address positioned pairs, so a silenced edge
    /// joins two positioned nodes and is never a step of a route.
    ///
    /// Where a pair is reachable more than one way, the shortest route is
    /// taken, and among equally short ones the one whose nodes sort first by
    /// qualified name, so the report names the same nodes on every run.
    fn dependencies_over_unpositioned(
        &self,
        positioned: &HashSet<NodeIndex>,
        is_violation: &impl Fn(NodeIndex, NodeIndex) -> bool,
    ) -> Vec<Dependency> {
        let graph = self.graph();
        // A `HashSet` iterates in a different order on every run; sorting the
        // sources fixes the order the report lists the pairs in.
        let mut sources: Vec<NodeIndex> = positioned.iter().copied().collect();
        sources.sort_by_cached_key(|&node| graph.qualified_name(node));
        sources
            .into_iter()
            .flat_map(|source| {
                self.routes_over_unpositioned(source, positioned)
                    .into_iter()
                    .filter(move |route| is_violation(source, route.target))
                    .map(move |route| Dependency {
                        source,
                        target: route.target,
                        carrier: Carrier::Path(written_edges(graph, source, &route)),
                    })
            })
            .collect()
    }

    /// Every route from `source` to a positioned node over at least one
    /// unpositioned node. The walk goes breadth first and enters each node
    /// once, so every route is a shortest one and no node is the target of
    /// two. A node `source` reaches directly gets no route: that is a written
    /// edge, which [`Self::written_dependencies`] already holds against the
    /// rule.
    fn routes_over_unpositioned(
        &self,
        source: NodeIndex,
        positioned: &HashSet<NodeIndex>,
    ) -> Vec<Route> {
        let graph = self.graph();
        let mut reached: HashSet<NodeIndex> = HashSet::from([source]);
        let mut frontier = vec![(source, Vec::new())];
        let mut found = Vec::new();
        while !frontier.is_empty() {
            let mut next: Vec<(NodeIndex, Vec<NodeIndex>)> = Vec::new();
            for (node, over) in frontier {
                let mut neighbors: Vec<NodeIndex> = graph
                    .edges(node)
                    .filter(|edge| edge.weight().is_production())
                    .map(|edge| edge.target())
                    .collect();
                // Fixes the order routes are found in, and with it the order
                // the report lists them in.
                neighbors.sort_by_cached_key(|&neighbor| graph.qualified_name(neighbor));
                for neighbor in neighbors {
                    if !reached.insert(neighbor) {
                        continue;
                    }
                    if positioned.contains(&neighbor) {
                        if !over.is_empty() {
                            found.push(Route {
                                target: neighbor,
                                over: over.clone(),
                            });
                        }
                        continue;
                    }
                    let mut onward = over.clone();
                    onward.push(neighbor);
                    next.push((neighbor, onward));
                }
            }
            // `reached` keeps the first route to a node. All entries of a
            // round are equally long, so name order here breaks the ties.
            next.sort_by_cached_key(|(_, over)| {
                over.iter()
                    .map(|&node| graph.qualified_name(node))
                    .collect::<Vec<_>>()
            });
            frontier = next;
        }
        found
    }

    /// The production edges `is_violation` rejects, one dependency each.
    fn written_dependencies(
        &self,
        is_violation: &impl Fn(NodeIndex, NodeIndex) -> bool,
    ) -> Vec<Dependency> {
        let graph = self.graph();
        graph
            .edge_indices()
            .filter_map(|edge_idx| {
                let weight = &graph[edge_idx];
                if !weight.is_production() {
                    return None;
                }
                let (source, target) = graph.edge_endpoints(edge_idx).expect("edge should exist");
                if !is_violation(source, target) {
                    return None;
                }
                Some(Dependency {
                    source,
                    target,
                    carrier: Carrier::Written(match weight {
                        EdgeWeight::ModuleDep { locations, .. } => locations.clone(),
                        _ => Vec::new(),
                    }),
                })
            })
            .collect()
    }

    /// Shared by all edge-predicate rule checks (forbidden-dependency, layers);
    /// only which dependencies reach here and the rule-type label differ
    /// between them. A dependency covered by `except` still produces a
    /// `Violation`, but lands in the allowed side. A baseline check runs only
    /// after that: `except` is a permanent allowance, the baseline a frozen
    /// one. Both are keyed on the pair, so a dependency running over
    /// intermediate nodes is silenced the same way a written edge is.
    fn check_dependency_violations(
        &self,
        rule: &Rule,
        except: &ResolvedExceptions,
        dependencies: Vec<Dependency>,
    ) -> CheckResult {
        let graph = self.graph();
        let mut reported = Vec::new();
        let mut allowed = Vec::new();
        let mut frozen = Vec::new();
        let mut baseline_entries = Vec::new();
        let mut baseline_hits = Vec::new();
        for Dependency {
            source,
            target,
            carrier,
        } in dependencies
        {
            let edge = Edge::new(graph.qualified_name(source), graph.qualified_name(target));
            let symbols = carrier.observed_symbols();
            if except.covers(source, target) {
                allowed.push(edge_violation(
                    rule,
                    &edge,
                    carrier,
                    None,
                    ViolationState::Allowed,
                ));
                continue;
            }
            let tolerated = self.baseline.frozen_for(&rule.name, &edge);
            let covered = tolerated.is_some_and(|t| t.covers(&symbols));
            let state = if covered {
                ViolationState::Frozen
            } else {
                ViolationState::Reported
            };
            let violation = edge_violation(
                rule,
                &edge,
                carrier,
                tolerated.filter(|_| !covered).cloned(),
                state,
            );
            let entry = BaselineEntry {
                rule: rule.name.clone(),
                key: ViolationKey { edge, symbols },
            };
            if tolerated.is_some() {
                baseline_hits.push(entry.clone());
            }
            if covered {
                frozen.push(violation);
            } else {
                baseline_entries.push(entry);
                reported.push(violation);
            }
        }
        let mut violations = reported;
        violations.extend(allowed);
        violations.extend(frozen);
        CheckResult {
            violations,
            baseline_entries,
            baseline_hits,
            diagnostics: Vec::new(),
            checked_rules: Vec::new(),
        }
    }

    /// Check all rules in `config` against the run.
    ///
    /// Diagnostics are raised after the rules: a stale baseline entry is only
    /// recognizable once every rule has had its chance to match it.
    ///
    /// # Errors
    /// `LayerOverlapError` if a `layers` rule's ordinary positions both claim
    /// one node.
    pub(crate) fn check_all(&self, config: &ArcConfig) -> Result<CheckResult, LayerOverlapError> {
        let checked: Vec<&Rule> = config
            .rules
            .iter()
            .filter(|rule| rule.severity != Severity::Ignore)
            .collect();

        let mut result: CheckResult = checked
            .iter()
            .map(|rule| self.check_rule(rule))
            .collect::<Result<Vec<CheckResult>, LayerOverlapError>>()?
            .into_iter()
            .collect();
        result.checked_rules = checked.iter().map(|rule| rule.name.clone()).collect();
        result.diagnostics = diagnostics::collect(
            &self.pattern_index,
            config,
            self.baseline,
            &result.baseline_hits,
        );
        Ok(result)
    }

    #[must_use]
    pub(crate) fn dead_excepts(&self, config: &ArcConfig) -> Vec<diagnostics::DeadExcept> {
        diagnostics::dead_excepts(&self.pattern_index, config)
    }
}

/// Set up a run over `graph` and check every rule in `config` against it.
///
/// # Errors
/// `LayerOverlapError` if a `layers` rule's ordinary positions both claim one
/// node.
pub fn check_rules(
    graph: &ArcGraph,
    config: &ArcConfig,
    baseline: &Baseline,
    include_reexports: bool,
) -> Result<CheckResult, LayerOverlapError> {
    CheckRun::new(graph, baseline, include_reexports).check_all(config)
}

/// `except` entries of a rule, resolved once to node sets so a per-edge check
/// is a set lookup rather than a re-run of `resolve_pattern` over the graph.
struct ResolvedExceptions(Vec<(HashSet<NodeIndex>, HashSet<NodeIndex>)>);

impl ResolvedExceptions {
    fn resolve(exceptions: &[Except], run: &CheckRun) -> Self {
        Self(
            exceptions
                .iter()
                .map(|exception| {
                    (
                        run.resolve_set(&exception.from),
                        run.resolve_set(&exception.to),
                    )
                })
                .collect(),
        )
    }

    /// Whether the rule carries no `except` entry at all, so per-edge work can
    /// be skipped entirely.
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether any exception's (from, to) pair covers this edge.
    fn covers(&self, source: NodeIndex, target: NodeIndex) -> bool {
        self.0
            .iter()
            .any(|(from_set, to_set)| from_set.contains(&source) && to_set.contains(&target))
    }
}

/// Remove the `excepted` edges from `subgraph`, keeping those that lay on a
/// cycle. An edge `(u, v)` lies on one exactly when `u` and `v` share a
/// strongly connected component of a graph that still holds the edge, so the
/// components are taken before the removal. Node weights of `subgraph` are the
/// original `NodeIndex` values, which is what the returned pairs use.
fn drop_excepted_edges(
    subgraph: &mut DiGraph<NodeIndex, ()>,
    excepted: Vec<(NodeIndex, NodeIndex)>,
) -> Vec<(NodeIndex, NodeIndex)> {
    if excepted.is_empty() {
        return Vec::new();
    }

    // Subgraph indices are dense, so the component id fits in a Vec slot.
    let mut scc_of = vec![usize::MAX; subgraph.node_count()];
    for (id, component) in tarjan_scc(&*subgraph).into_iter().enumerate() {
        for node in component {
            scc_of[node.index()] = id;
        }
    }
    let sub_of: HashMap<NodeIndex, NodeIndex> = subgraph
        .node_indices()
        .map(|node| (subgraph[node], node))
        .collect();

    let mut on_cycle = Vec::new();
    for (source, target) in excepted {
        let (sub_source, sub_target) = (sub_of[&source], sub_of[&target]);
        if let Some(edge_idx) = subgraph.find_edge(sub_source, sub_target) {
            subgraph.remove_edge(edge_idx);
        }
        if scc_of[sub_source.index()] == scc_of[sub_target.index()] {
            on_cycle.push((source, target));
        }
    }
    on_cycle
}

/// Source locations of the production `ModuleDep` edge between `source` and
/// `target`, empty when there is none. The graph holds at most one `ModuleDep`
/// per node pair, so the lookup is unambiguous.
fn module_dep_locations(
    graph: &ArcGraph,
    source: NodeIndex,
    target: NodeIndex,
) -> Vec<SourceLocation> {
    graph
        .edges_connecting(source, target)
        .find_map(|edge| match edge.weight() {
            EdgeWeight::ModuleDep { locations, .. } => Some(locations.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;
    use crate::model::EdgeContext;
    use crate::rules::config::Diagnostics;
    use crate::rules::diagnostics::{DiagnosticKind, UnsortedNode};
    use std::path::PathBuf;

    // -- Test graph helpers --

    fn check_rule(graph: &ArcGraph, rule: &Rule, include_reexports: bool) -> CheckResult {
        check_rule_with_baseline(graph, rule, include_reexports, &Baseline::empty())
    }

    fn check_rule_with_baseline(
        graph: &ArcGraph,
        rule: &Rule,
        include_reexports: bool,
        baseline: &Baseline,
    ) -> CheckResult {
        CheckRun::new(graph, baseline, include_reexports)
            .check_rule(rule)
            .expect("no layer overlap in this test graph")
    }

    /// Writes `entries` to a throwaway `arc-baseline.toml` and loads it back,
    /// the only way to get a populated [`Baseline`] (its fields are private).
    fn baseline_of(entries: &[BaselineEntry]) -> Baseline {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, entries).unwrap();
        Baseline::load(&path).unwrap()
    }

    fn test_crate_graph() -> (ArcGraph, NodeIndex) {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "test".into(),
            path: PathBuf::from("/test"),
        });
        (graph, crate_idx)
    }

    fn add_module(
        graph: &mut ArcGraph,
        name: &str,
        crate_idx: NodeIndex,
        parent: NodeIndex,
    ) -> NodeIndex {
        let idx = graph.add_node(Node::Module {
            name: name.into(),
            crate_idx,
        });
        graph.add_edge(parent, idx, EdgeWeight::Contains);
        idx
    }

    fn add_production_dep(graph: &mut ArcGraph, from: NodeIndex, to: NodeIndex) {
        graph.add_edge(
            from,
            to,
            EdgeWeight::ModuleDep {
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/lib.rs"),
                    line: 1,
                    symbols: vec![],
                    module_path: String::new(),
                    via_reexport: false,
                }],
                context: EdgeContext::production(),
            },
        );
    }

    fn add_named_dep(graph: &mut ArcGraph, from: NodeIndex, to: NodeIndex, symbols: &[&str]) {
        graph.add_edge(
            from,
            to,
            EdgeWeight::ModuleDep {
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/lib.rs"),
                    line: 1,
                    symbols: symbols.iter().map(|s| (*s).to_string()).collect(),
                    module_path: String::new(),
                    via_reexport: false,
                }],
                context: EdgeContext::production(),
            },
        );
    }

    fn named(symbols: &[&str]) -> EdgeSymbols {
        EdgeSymbols {
            named: symbols.iter().map(|s| (*s).to_string()).collect(),
            bare: false,
        }
    }

    /// A baseline entry for an edge built by [`add_production_dep`], whose
    /// single location names no symbol.
    fn frozen_edge(rule: &str, from: &str, to: &str) -> BaselineEntry {
        BaselineEntry {
            rule: rule.to_string(),
            key: ViolationKey {
                edge: Edge::new(from, to),
                symbols: EdgeSymbols {
                    bare: true,
                    ..EdgeSymbols::default()
                },
            },
        }
    }

    fn named_frozen_edge(rule: &str, from: &str, to: &str, symbols: &[&str]) -> BaselineEntry {
        BaselineEntry {
            rule: rule.to_string(),
            key: ViolationKey {
                edge: Edge::new(from, to),
                symbols: named(symbols),
            },
        }
    }

    fn add_reexport_dep(graph: &mut ArcGraph, from: NodeIndex, to: NodeIndex) {
        graph.add_edge(
            from,
            to,
            EdgeWeight::ModuleDep {
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/lib.rs"),
                    line: 1,
                    symbols: vec![],
                    module_path: String::new(),
                    via_reexport: true,
                }],
                context: EdgeContext::production(),
            },
        );
    }

    fn add_test_dep(graph: &mut ArcGraph, from: NodeIndex, to: NodeIndex) {
        use crate::model::TestKind;
        graph.add_edge(
            from,
            to,
            EdgeWeight::ModuleDep {
                locations: vec![],
                context: EdgeContext::test(TestKind::Unit),
            },
        );
    }

    /// Build a multi-crate graph:
    /// - crate "domain" with modules: service, model
    /// - crate "infra" with modules: db, api
    /// - crate "application" with module: handler
    fn multi_crate_graph() -> (
        ArcGraph,
        NodeIndex,
        NodeIndex,
        NodeIndex,
        NodeIndex,
        NodeIndex,
        NodeIndex,
        NodeIndex,
        NodeIndex,
    ) {
        let mut graph = ArcGraph::new();

        let domain = graph.add_node(Node::Crate {
            name: "domain".into(),
            path: PathBuf::from("/domain"),
        });
        let service = add_module(&mut graph, "service", domain, domain);
        let model = add_module(&mut graph, "model", domain, domain);

        let infra = graph.add_node(Node::Crate {
            name: "infra".into(),
            path: PathBuf::from("/infra"),
        });
        let db = add_module(&mut graph, "db", infra, infra);
        let api = add_module(&mut graph, "api", infra, infra);

        let application = graph.add_node(Node::Crate {
            name: "application".into(),
            path: PathBuf::from("/application"),
        });
        let handler = add_module(&mut graph, "handler", application, application);

        (
            graph,
            domain,
            service,
            model,
            infra,
            db,
            api,
            application,
            handler,
        )
    }

    // ===== Task 2.1: forbidden-dependency tests =====

    #[test]
    fn test_forbidden_violation_found() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // domain::service → infra::db (forbidden)
        add_production_dep(&mut graph, service, db);

        let result = check_rule(&graph, &no_infra_in_domain(vec![]), false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].rule_name, "no infra in domain");
        assert_eq!(reported[0].rule_type, "forbidden-dependency");
        let ViolationDetail::Edge { edge, .. } = &reported[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(edge.from.contains("service"));
        assert!(edge.to.contains("db"));
    }

    #[test]
    fn test_forbidden_no_violation() {
        let (mut graph, _domain, service, _model, _infra, _db, _api, _application, handler) =
            multi_crate_graph();
        // domain::service → application::handler (allowed, rule forbids domain→infra)
        add_production_dep(&mut graph, service, handler);

        let result = check_rule(&graph, &no_infra_in_domain(vec![]), false);
        let reported: Vec<_> = result.reported().collect();
        assert!(reported.is_empty());
    }

    #[test]
    fn test_forbidden_multiple_violations() {
        let (mut graph, _domain, service, model, _infra, db, api, _app, _handler) =
            multi_crate_graph();
        // Two forbidden edges: service→db and model→api
        add_production_dep(&mut graph, service, db);
        add_production_dep(&mut graph, model, api);

        let result = check_rule(&graph, &no_infra_in_domain(vec![]), false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 2);
    }

    #[test]
    fn test_forbidden_ignores_test_edges() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // Test-only edge: should not trigger violation
        add_test_dep(&mut graph, service, db);

        let result = check_rule(&graph, &no_infra_in_domain(vec![]), false);
        let reported: Vec<_> = result.reported().collect();
        assert!(reported.is_empty());
    }

    /// Multi-crate graph carrying the single production edge
    /// `domain::service → infra::db`, the one the rule below reports on.
    fn service_to_db_graph() -> ArcGraph {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, service, db);
        graph
    }

    /// `forbidden-dependency` rule `domain::** → infra::**` with the given
    /// `except` entries.
    fn no_infra_in_domain(except: Vec<Except>) -> Rule {
        Rule {
            name: "no infra in domain".into(),
            severity: Severity::Error,
            except,
            kind: RuleKind::ForbiddenDependency(ForbiddenDependencyRule {
                from: "domain::**".into(),
                to: "infra::**".into(),
            }),
        }
    }

    /// `no-cycles` rule over `scope` with the given `except` entries.
    fn no_cycles_rule(name: &str, scope: &str, except: Vec<Except>) -> Rule {
        Rule {
            name: name.into(),
            severity: Severity::Error,
            except,
            kind: RuleKind::NoCycles(NoCyclesRule {
                scope: scope.into(),
            }),
        }
    }

    /// Top-down `layers` rule over the given layer patterns.
    fn layers_rule(layers: &[&str], except: Vec<Except>) -> Rule {
        Rule {
            name: "architecture layers".into(),
            severity: Severity::Error,
            except,
            kind: RuleKind::Layers(LayersRule {
                layers: layers.iter().map(|&layer| layer.into()).collect(),
                direction: Direction::TopDown,
                exhaustive: false,
            }),
        }
    }

    /// `ArcConfig` over `rules`, with the default diagnostic levels.
    fn config_of(rules: Vec<Rule>) -> ArcConfig {
        ArcConfig {
            rules,
            diagnostics: Diagnostics::default(),
        }
    }

    fn except_edge(from: &str, to: &str) -> Except {
        Except {
            from: from.into(),
            to: to.into(),
            reason: None,
        }
    }

    #[test]
    fn test_forbidden_except_allows_matching_edge() {
        let rule = no_infra_in_domain(vec![except_edge("domain::service", "infra::db")]);
        let result = check_rule(&service_to_db_graph(), &rule, false);
        let allowed: Vec<_> = result.allowed().collect();
        assert!(result.reported().next().is_none());
        assert_eq!(allowed.len(), 1);
        let ViolationDetail::Edge { edge, .. } = &allowed[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(edge.from.contains("service"));
        assert!(edge.to.contains("db"));
    }

    #[test]
    fn test_forbidden_except_not_matching_leaves_violation_reported() {
        let rule = no_infra_in_domain(vec![except_edge("domain::model", "infra::db")]);
        let result = check_rule(&service_to_db_graph(), &rule, false);
        assert_eq!(result.reported().count(), 1);
        assert!(result.allowed().next().is_none());
    }

    #[test]
    fn test_forbidden_except_wildcard_matches_inside_a_segment() {
        let rule = no_infra_in_domain(vec![except_edge("domain::serv*", "infra::*b")]);
        let result = check_rule(&service_to_db_graph(), &rule, false);
        assert!(result.reported().next().is_none());
        assert_eq!(result.allowed().count(), 1);
    }

    #[test]
    fn test_forbidden_except_pattern_matches_glob() {
        let rule = no_infra_in_domain(vec![except_edge("domain::**", "infra::**")]);
        let result = check_rule(&service_to_db_graph(), &rule, false);
        assert!(result.reported().next().is_none());
        assert_eq!(result.allowed().count(), 1);
    }

    // ===== Task 2.2: no-cycles tests =====

    #[test]
    fn test_cycles_in_scope() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // Cycle: a → b → a
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1);
        let ViolationDetail::Cluster(cluster) = &reported[0].detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 1);
        assert!(cluster.cycle.is_some());
    }

    #[test]
    fn test_pure_reexport_cycle_ignored_by_default() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // a re-exports from b (pub use), b uses a behaviorally. The cycle exists
        // only through the re-export edge → idiomatic, not real coupling.
        add_reexport_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, a);

        let rule = no_cycles_rule("no cycles", "test::**", vec![]);
        // Default: the idiomatic re-export cycle is not reported (ADR-022).
        assert!(
            check_rule(&graph, &rule, false).reported().next().is_none(),
            "pure re-export cycle should be ignored by default"
        );
        // --include-reexports opts back into the full graph and surfaces it.
        assert_eq!(
            check_rule(&graph, &rule, true).reported().count(),
            1,
            "include_reexports should surface the re-export cycle"
        );
    }

    #[test]
    fn test_cycles_outside_scope() {
        let (mut graph, _domain, _service, _model, _infra, db, api, _app, _handler) =
            multi_crate_graph();
        // Cycle in infra: db → api → db
        add_production_dep(&mut graph, db, api);
        add_production_dep(&mut graph, api, db);

        // Rule scoped to domain only — should not find infra cycle
        let rule = no_cycles_rule("no cycles in domain", "domain::**", vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert!(reported.is_empty());
    }

    #[test]
    fn test_no_cycles() {
        let (mut graph, _domain, service, model, _infra, _db, _api, _app, _handler) =
            multi_crate_graph();
        // Linear: service → model (no cycle)
        add_production_dep(&mut graph, service, model);

        let rule = no_cycles_rule("no cycles in domain", "domain::**", vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert!(reported.is_empty());
    }

    #[test]
    fn test_cycles_global_scope() {
        let (mut graph, _domain, _service, _model, _infra, db, api, _app, _handler) =
            multi_crate_graph();
        // Add two more modules in separate crates for two independent cycles
        let crate_a = graph
            .node_indices()
            .find(|&i| graph[i].name() == "domain")
            .unwrap();
        let a1 = add_module(&mut graph, "x", crate_a, crate_a);
        let a2 = add_module(&mut graph, "y", crate_a, crate_a);
        add_production_dep(&mut graph, a1, a2);
        add_production_dep(&mut graph, a2, a1);
        add_production_dep(&mut graph, db, api);
        add_production_dep(&mut graph, api, db);

        let rule = no_cycles_rule("global no-cycles", "**", vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 2);
    }

    #[test]
    fn test_cycles_one_violation_per_cluster() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let c = add_module(&mut graph, "c", crate_idx, crate_idx);
        let d = add_module(&mut graph, "d", crate_idx, crate_idx);
        // Two triangles sharing edge a -> b: one SCC, two cycles.
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, c);
        add_production_dep(&mut graph, c, a);
        add_production_dep(&mut graph, b, d);
        add_production_dep(&mut graph, d, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1);
        let ViolationDetail::Cluster(cluster) = &reported[0].detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 2);
        assert!(cluster.cycle.is_none());
    }

    #[test]
    fn test_cycles_except_removes_matching_edge_before_search() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // Cycle: a → b → a, but b → a is excepted.
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, a);

        let rule = no_cycles_rule(
            "no cycles in test",
            "test::**",
            vec![except_edge("test::b", "test::a")],
        );
        let result = check_rule(&graph, &rule, false);
        let allowed: Vec<_> = result.allowed().collect();
        assert!(
            result.reported().next().is_none(),
            "except should remove the edge before the cycle can form"
        );
        assert_eq!(allowed.len(), 1);
        let ViolationDetail::Edge { edge, .. } = &allowed[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(edge.from.contains('b'));
        assert!(edge.to.contains('a'));
        assert_eq!(
            allowed[0].locations.len(),
            1,
            "the excepted edge's source locations belong on the allowed violation"
        );
    }

    #[test]
    fn test_cycles_except_on_edge_off_any_cycle_is_not_recorded() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // a → b is the only edge: nothing here ever forms a cycle.
        add_production_dep(&mut graph, a, b);

        let rule = no_cycles_rule(
            "no cycles in test",
            "test::**",
            vec![except_edge("test::a", "test::b")],
        );
        let result = check_rule(&graph, &rule, false);
        assert!(result.reported().next().is_none());
        assert!(
            result.allowed().next().is_none(),
            "an edge that never lay on a cycle is not an allowed violation"
        );
    }

    #[test]
    fn test_common_place_nested_modules_share_prefix() {
        let (mut graph, crate_idx) = test_crate_graph();
        let back = add_module(&mut graph, "back", crate_idx, crate_idx);
        let hlsl = add_module(&mut graph, "hlsl", crate_idx, back);
        let writer = add_module(&mut graph, "writer", crate_idx, hlsl);
        let keywords = add_module(&mut graph, "keywords", crate_idx, hlsl);
        assert_eq!(
            common_place(&graph, &[writer, keywords]),
            "test::back::hlsl"
        );
    }

    #[test]
    fn test_common_place_flat_modules_share_only_the_crate() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        assert_eq!(common_place(&graph, &[a, b]), "test");
    }

    // ===== Task 2.3: layers tests =====

    #[test]
    fn test_layers_valid_top_down() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // domain::service → infra::db (top-down: domain is higher layer)
        add_production_dep(&mut graph, service, db);

        let rule = layers_rule(&["domain", "application", "infra"], vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert!(reported.is_empty());
    }

    #[test]
    fn test_layers_violation_bottom_up() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // infra::db → domain::service (bottom-up in top-down rule = violation)
        add_production_dep(&mut graph, db, service);

        let rule = layers_rule(&["domain", "application", "infra"], vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1);
        let ViolationDetail::Edge { edge, .. } = &reported[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(edge.from.contains("db"));
        assert!(edge.to.contains("service"));
    }

    #[test]
    fn test_layers_skip_layer() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // domain::service → infra::db (skipping application layer — allowed in top-down)
        add_production_dep(&mut graph, service, db);

        let rule = layers_rule(&["domain", "application", "infra"], vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert!(reported.is_empty());
    }

    /// Top-down `layers` rule whose ranks may hold several patterns each.
    fn layers_rule_of_ranks(ranks: &[&[&str]]) -> Rule {
        Rule {
            name: "architecture layers".into(),
            severity: Severity::Error,
            except: vec![],
            kind: RuleKind::Layers(LayersRule {
                layers: ranks
                    .iter()
                    .map(|rank| rank.iter().map(|&p| p.to_owned()).collect())
                    .collect(),
                direction: Direction::TopDown,
                exhaustive: false,
            }),
        }
    }

    #[test]
    fn test_layers_equal_rank_edge_is_not_a_violation() {
        let (mut graph, _domain, _service, _model, _infra, db, _api, _app, handler) =
            multi_crate_graph();
        // infra::db → application::handler, both on the same rank
        add_production_dep(&mut graph, db, handler);

        let rule = layers_rule_of_ranks(&[&["domain"], &["infra", "application"]]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert!(
            reported.is_empty(),
            "crates of equal rank may depend on each other: {reported:?}"
        );
    }

    /// Sharing a rank must not exempt its members from the ordering itself.
    #[test]
    fn test_layers_equal_rank_still_bound_by_the_ordering() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // infra::db → domain::service, upward out of the shared rank
        add_production_dep(&mut graph, db, service);

        let rule = layers_rule_of_ranks(&[&["domain"], &["infra", "application"]]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1);
    }

    /// The order within a rank carries no meaning: reversing it must not change
    /// the verdict. This is the defect that made equal-rank crates unexpressible.
    #[test]
    fn test_layers_order_within_a_rank_does_not_matter() {
        let (mut graph, _domain, _service, _model, _infra, db, _api, _app, handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, db, handler);

        for rank in [["infra", "application"], ["application", "infra"]] {
            let rule = layers_rule_of_ranks(&[&["domain"], &rank]);
            let result = check_rule(&graph, &rule, false);
            let reported: Vec<_> = result.reported().collect();
            assert!(reported.is_empty(), "order {rank:?} changed the verdict");
        }
    }

    #[test]
    fn test_layers_outside_layers() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // Edge between modules not in any defined layer — should be ignored
        add_production_dep(&mut graph, a, b);

        let rule = layers_rule(&["domain", "infra"], vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert!(reported.is_empty());
    }

    /// One crate node per name in `crates`, plus a production dependency for
    /// every pair in `deps`. Both take names rather than indices, because the
    /// layers tests below assert on names.
    fn crate_graph(crates: &[&str], deps: &[(&str, &str)]) -> ArcGraph {
        let mut graph = ArcGraph::new();
        let mut index: HashMap<&str, NodeIndex> = HashMap::new();
        for &name in crates {
            let node = graph.add_node(Node::Crate {
                name: name.into(),
                path: PathBuf::from(format!("/{name}")),
            });
            index.insert(name, node);
        }
        for &(from, to) in deps {
            add_production_dep(&mut graph, index[from], index[to]);
        }
        graph
    }

    /// `a → b → c → d`, one dependency each. No rule below positions `c`, so
    /// `b` reaches `d` without an edge saying so.
    fn chain_of_four_crates() -> ArcGraph {
        crate_graph(&["a", "b", "c", "d"], &[("a", "b"), ("b", "c"), ("c", "d")])
    }

    /// The edges a violation runs through, as `(from, to)` pairs.
    fn via_pairs(violation: &Violation) -> Vec<(&str, &str)> {
        let ViolationDetail::Edge { via, .. } = &violation.detail else {
            panic!("expected an edge detail");
        };
        via.iter()
            .map(|written| (written.edge.from.as_str(), written.edge.to.as_str()))
            .collect()
    }

    /// The pair of a violation's edge.
    fn violation_pair(violation: &Violation) -> (&str, &str) {
        let ViolationDetail::Edge { edge, .. } = &violation.detail else {
            panic!("expected an edge detail");
        };
        (edge.from.as_str(), edge.to.as_str())
    }

    #[test]
    fn test_layers_dependency_over_an_unpositioned_crate_is_a_violation() {
        let rule = layers_rule(&["a", "d", "b"], vec![]);
        let result = check_rule(&chain_of_four_crates(), &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1, "one pair, one violation: {reported:?}");
        assert_eq!(violation_pair(reported[0]), ("b", "d"));
        assert_eq!(via_pairs(reported[0]), [("b", "c"), ("c", "d")]);
    }

    /// The same crates under an order they obey. Reaching `d` over `c` is no
    /// more a violation than reaching it directly.
    #[test]
    fn test_layers_dependency_over_an_unpositioned_crate_may_run_downward() {
        let rule = layers_rule(&["a", "b", "d"], vec![]);
        let result = check_rule(&chain_of_four_crates(), &rule, false);
        assert!(result.reported().next().is_none());
    }

    /// The same pair reachable over either of two crates. The report takes the
    /// crate that sorts first, on every run.
    #[test]
    fn test_layers_equal_path_lengths_resolve_by_name() {
        let graph = crate_graph(
            &["b", "d", "k", "m"],
            &[("b", "k"), ("k", "d"), ("b", "m"), ("m", "d")],
        );
        let rule = layers_rule(&["d", "b"], vec![]);
        for _ in 0..10 {
            let result = check_rule(&graph, &rule, false);
            let reported: Vec<_> = result.reported().collect();
            assert_eq!(reported.len(), 1, "one pair, one violation: {reported:?}");
            assert_eq!(via_pairs(reported[0]), [("b", "k"), ("k", "d")]);
        }
    }

    /// Two edges beat three, even when the crates of the longer way sort first
    /// by name.
    #[test]
    fn test_layers_the_longer_way_is_not_reported() {
        let graph = crate_graph(
            &["b", "d", "p", "q", "z"],
            &[("b", "z"), ("z", "d"), ("b", "p"), ("p", "q"), ("q", "d")],
        );
        let rule = layers_rule(&["d", "b"], vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1, "one pair, one violation: {reported:?}");
        assert_eq!(via_pairs(reported[0]), [("b", "z"), ("z", "d")]);
    }

    /// A written edge is reported as itself. It brings the location of its
    /// import, and nothing it runs through.
    #[test]
    fn test_layers_written_edge_wins_over_the_longer_path() {
        let graph = crate_graph(&["b", "c", "d"], &[("b", "c"), ("c", "d"), ("b", "d")]);
        let rule = layers_rule(&["d", "b"], vec![]);
        let result = check_rule(&graph, &rule, false);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1, "one pair, one violation: {reported:?}");
        assert_eq!(violation_pair(reported[0]), ("b", "d"));
        assert!(
            via_pairs(reported[0]).is_empty(),
            "the edge carries the dependency itself"
        );
        assert!(
            !reported[0].locations.is_empty(),
            "the written edge brings its locations"
        );
    }

    /// A "domain" crate plus two crates that do not match `*domain*`.
    fn domain_and_others_graph() -> (ArcGraph, NodeIndex, NodeIndex, NodeIndex) {
        let mut graph = ArcGraph::new();
        let domain = graph.add_node(Node::Crate {
            name: "domain".into(),
            path: PathBuf::from("/domain"),
        });
        let svc_orders = graph.add_node(Node::Crate {
            name: "svc_orders".into(),
            path: PathBuf::from("/svc_orders"),
        });
        let svc_billing = graph.add_node(Node::Crate {
            name: "svc_billing".into(),
            path: PathBuf::from("/svc_billing"),
        });
        (graph, domain, svc_orders, svc_billing)
    }

    /// Top-down `layers = [["*"], ["*domain*"]]`: the catch-all above domain.
    fn catch_all_above_domain_rule() -> Rule {
        layers_rule_of_ranks(&[&["*"], &["*domain*"]])
    }

    #[test]
    fn test_layers_catch_all_domain_depending_downward_is_a_violation() {
        let (mut graph, domain, svc_orders, _svc_billing) = domain_and_others_graph();
        add_production_dep(&mut graph, domain, svc_orders);

        let result = check_rule(&graph, &catch_all_above_domain_rule(), false);
        assert_eq!(result.reported().count(), 1);
    }

    #[test]
    fn test_layers_catch_all_may_depend_on_domain() {
        let (mut graph, domain, svc_orders, _svc_billing) = domain_and_others_graph();
        add_production_dep(&mut graph, svc_orders, domain);

        let result = check_rule(&graph, &catch_all_above_domain_rule(), false);
        assert!(result.reported().next().is_none());
    }

    #[test]
    fn test_layers_catch_all_edge_inside_domain_is_not_a_violation() {
        let (mut graph, domain, ..) = domain_and_others_graph();
        let inner = add_module(&mut graph, "inner", domain, domain);
        add_production_dep(&mut graph, domain, inner);

        let result = check_rule(&graph, &catch_all_above_domain_rule(), false);
        assert!(result.reported().next().is_none());
    }

    #[test]
    fn test_catch_all_needs_no_rule_change_when_a_crate_is_added() {
        let rule = catch_all_above_domain_rule();

        let mut before = ArcGraph::new();
        let domain = before.add_node(Node::Crate {
            name: "domain".into(),
            path: PathBuf::from("/domain"),
        });
        let svc_orders = before.add_node(Node::Crate {
            name: "svc_orders".into(),
            path: PathBuf::from("/svc_orders"),
        });
        add_production_dep(&mut before, domain, svc_orders);
        assert_eq!(check_rule(&before, &rule, false).reported().count(), 1);

        // Same rule, a graph that really adds svc_billing.
        let (mut after, domain2, svc_orders2, svc_billing2) = domain_and_others_graph();
        add_production_dep(&mut after, domain2, svc_orders2);
        add_production_dep(&mut after, domain2, svc_billing2);
        let result = check_rule(&after, &rule, false);
        assert_eq!(
            result.reported().count(),
            2,
            "the crate added after the rule was written still falls under the catch-all"
        );
    }

    #[test]
    fn test_layer_overlap_names_the_rules_file() {
        let overlap = LayerOverlapError {
            rule: "architecture layers".into(),
            node: "domain".into(),
            first: vec!["domain*".into()],
            second: vec!["*main".into()],
        };
        assert_eq!(
            overlap.in_file(Path::new("arc-rules.toml")).to_string(),
            "rule \"architecture layers\" in arc-rules.toml: \"domain\" matches both layer [\"domain*\"] and layer [\"*main\"]; \
             a layers rule gives a node one position, so split it into two rules"
        );
    }

    /// Both positions match the crate itself, which pulls its modules along:
    /// three nodes overlap, not one, so a run naming an arbitrary one of them
    /// would disagree with itself between runs.
    #[test]
    fn test_layers_overlapping_ordinary_positions_fail_the_run() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "svc_application_orders".into(),
            path: PathBuf::from("/svc_application_orders"),
        });
        add_module(&mut graph, "alpha", crate_idx, crate_idx);
        add_module(&mut graph, "beta", crate_idx, crate_idx);

        let rule = layers_rule_of_ranks(&[&["*application*"], &["*orders*"]]);
        let err = CheckRun::new(&graph, &Baseline::empty(), false)
            .check_rule(&rule)
            .expect_err("a node matched by two ordinary positions must fail the run");
        assert_eq!(
            err.to_string(),
            "rule \"architecture layers\": \"svc_application_orders\" matches both layer [\"*application*\"] and layer [\"*orders*\"]; \
             a layers rule gives a node one position, so split it into two rules"
        );
    }

    #[test]
    fn test_layers_except_allows_matching_edge() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // infra::db → domain::service (bottom-up in top-down rule), but excepted.
        add_production_dep(&mut graph, db, service);

        let rule = layers_rule(
            &["domain", "application", "infra"],
            vec![except_edge("infra::db", "domain::service")],
        );
        let result = check_rule(&graph, &rule, false);
        assert!(result.reported().next().is_none());
        assert_eq!(result.allowed().count(), 1);
    }

    // ===== Task 2.4: orchestration tests =====

    #[test]
    fn test_check_rules_mixed() {
        let (mut graph, _domain, service, _model, _infra, db, api, _app, _handler) =
            multi_crate_graph();
        // Forbidden: service → db
        add_production_dep(&mut graph, service, db);
        // Cycle: db → api → db
        add_production_dep(&mut graph, db, api);
        add_production_dep(&mut graph, api, db);

        let config = config_of(vec![
            no_infra_in_domain(vec![]),
            Rule {
                name: "no cycles in infra".into(),
                severity: Severity::Warn,
                except: vec![],
                kind: RuleKind::NoCycles(NoCyclesRule {
                    scope: "infra::**".into(),
                }),
            },
        ]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false).unwrap();
        assert_eq!(result.reported().count(), 2);
        assert!(
            result
                .reported()
                .any(|v| v.rule_type == "forbidden-dependency")
        );
        assert!(result.reported().any(|v| v.rule_type == "no-cycles"));
    }

    #[test]
    fn test_check_rules_empty() {
        let (graph, _) = test_crate_graph();
        let config = config_of(vec![]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false).unwrap();
        assert!(result.reported().next().is_none());
    }

    #[test]
    fn test_check_result_has_errors() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                state: ViolationState::Reported,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("a", "b"),
                    frozen_for: None,
                    via: Vec::new(),
                },
                locations: vec![],
            }],
            ..Default::default()
        };
        assert!(result.has_errors());
        assert!(result.has_negative_judgment());
    }

    #[test]
    fn test_has_errors_ignores_a_frozen_error_violation() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                state: ViolationState::Frozen,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("a", "b"),
                    frozen_for: None,
                    via: Vec::new(),
                },
                locations: vec![],
            }],
            ..Default::default()
        };
        assert!(!result.has_errors());
        assert!(!result.has_negative_judgment());
    }

    #[test]
    fn test_check_result_only_warnings() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "no-cycles".into(),
                severity: Severity::Warn,
                state: ViolationState::Reported,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("a", "b"),
                    frozen_for: None,
                    via: Vec::new(),
                },
                locations: vec![],
            }],
            ..Default::default()
        };
        assert!(!result.has_errors());
        assert!(!result.has_negative_judgment());
    }

    #[test]
    fn test_check_result_exit_code_ignores_allowed_error() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                state: ViolationState::Allowed,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("a", "b"),
                    frozen_for: None,
                    via: Vec::new(),
                },
                locations: vec![],
            }],
            ..Default::default()
        };
        assert!(!result.has_errors());
        assert!(!result.has_negative_judgment());
    }

    #[test]
    fn test_check_rules_reports_a_crate_an_exhaustive_rule_leaves_unsorted() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, service, db);

        // The layers rule names two of the three crates and declares itself
        // exhaustive, so the third is a gap.
        let mut rule = layers_rule(&["infra", "domain"], vec![]);
        let RuleKind::Layers(params) = &mut rule.kind else {
            unreachable!("layers_rule builds a layers rule")
        };
        params.exhaustive = true;
        let config = config_of(vec![rule]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false).unwrap();
        let unlayered: Vec<&str> = result
            .diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::UnlayeredNode { entry } => Some(entry.node.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(unlayered, ["application"]);
    }

    #[test]
    fn test_a_frozen_violation_counts_as_a_baseline_hit() {
        let rule = no_infra_in_domain(vec![]);
        let entry = frozen_edge(&rule.name, "domain::service", "infra::db");
        let baseline = baseline_of(std::slice::from_ref(&entry));
        let result = check_rule_with_baseline(&service_to_db_graph(), &rule, false, &baseline);
        assert_eq!(result.baseline_hits.len(), 1);
        assert_eq!(result.baseline_hits[0].rule, entry.rule);
        assert_eq!(result.baseline_hits[0].key, entry.key);
    }

    #[test]
    fn test_a_frozen_cycle_counts_one_hit_per_edge() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let baseline = baseline_of(&[
            frozen_edge(&rule.name, "test::a", "test::b"),
            frozen_edge(&rule.name, "test::b", "test::a"),
        ]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert_eq!(result.baseline_hits.len(), 2);
        assert!(result.reported().next().is_none());
        assert!(!result.has_negative_judgment());
    }

    #[test]
    fn test_one_uncovered_edge_keeps_the_cluster_reported() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let baseline = baseline_of(&[frozen_edge(&rule.name, "test::a", "test::b")]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1);
        assert!(matches!(reported[0].detail, ViolationDetail::Cluster(_)));
    }

    #[test]
    fn test_a_frozen_cycle_edge_that_gains_a_symbol_turns_red() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        add_named_dep(&mut graph, a, b, &["Old", "New"]);
        add_named_dep(&mut graph, b, a, &["Back"]);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let baseline = baseline_of(&[
            named_frozen_edge(&rule.name, "test::a", "test::b", &["Old"]),
            named_frozen_edge(&rule.name, "test::b", "test::a", &["Back"]),
        ]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);

        let reported: Vec<&Violation> = result.reported().collect();
        let outgrown: Vec<&&Violation> = reported
            .iter()
            .filter(|v| matches!(&v.detail, ViolationDetail::Edge { frozen_for, .. } if frozen_for.is_some()))
            .collect();
        assert_eq!(outgrown.len(), 1, "got: {reported:?}");
        let ViolationDetail::Edge { frozen_for, .. } = &outgrown[0].detail else {
            unreachable!("filtered for edges")
        };
        assert_eq!(frozen_for.as_ref().unwrap(), &named(&["Old"]));
        assert!(result.has_negative_judgment());
    }

    #[test]
    fn test_leaving_any_single_edge_uncovered_keeps_a_cluster_reported() {
        // The verdict per cycle is derived from the covered edges, so it holds
        // only if every SCC-internal edge lies on a representative cycle.
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let c = add_module(&mut graph, "c", crate_idx, crate_idx);
        let d = add_module(&mut graph, "d", crate_idx, crate_idx);
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, c);
        add_production_dep(&mut graph, c, a);
        add_production_dep(&mut graph, b, d);
        add_production_dep(&mut graph, d, a);
        let edges = [
            ("test::a", "test::b"),
            ("test::b", "test::c"),
            ("test::c", "test::a"),
            ("test::b", "test::d"),
            ("test::d", "test::a"),
        ];

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let all_frozen: Vec<BaselineEntry> = edges
            .iter()
            .map(|(from, to)| frozen_edge(&rule.name, from, to))
            .collect();
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline_of(&all_frozen));
        assert!(
            result.reported().next().is_none(),
            "every edge frozen leaves nothing"
        );

        for left_out in &edges {
            let rest: Vec<BaselineEntry> = edges
                .iter()
                .filter(|edge| *edge != left_out)
                .map(|(from, to)| frozen_edge(&rule.name, from, to))
                .collect();
            let result = check_rule_with_baseline(&graph, &rule, false, &baseline_of(&rest));
            assert_eq!(
                result.reported().count(),
                1,
                "leaving {left_out:?} uncovered must keep its cluster reported"
            );
        }
    }

    #[test]
    fn test_a_denied_diagnostic_fails_the_run() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Deny,
                kind: DiagnosticKind::UnlayeredNode {
                    entry: UnsortedNode {
                        rule: "architecture layers".into(),
                        node: "xtask".into(),
                    },
                },
            }],
            ..Default::default()
        };
        assert!(!result.has_errors(), "no rule was violated");
        assert!(result.has_negative_judgment());
    }

    #[test]
    fn test_a_warned_diagnostic_leaves_the_run_green() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warn,
                kind: DiagnosticKind::UnlayeredNode {
                    entry: UnsortedNode {
                        rule: "architecture layers".into(),
                        node: "xtask".into(),
                    },
                },
            }],
            ..Default::default()
        };
        assert!(!result.has_negative_judgment());
    }

    #[test]
    fn test_severity_ignore_filtered() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, service, db);

        let config = config_of(vec![Rule {
            name: "ignored rule".into(),
            severity: Severity::Ignore,
            except: vec![],
            kind: RuleKind::ForbiddenDependency(ForbiddenDependencyRule {
                from: "domain::**".into(),
                to: "infra::**".into(),
            }),
        }]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false).unwrap();
        assert!(result.reported().next().is_none());
    }

    // ===== Baseline tests =====

    #[test]
    fn test_frozen_edge_is_not_a_violation() {
        let rule = no_infra_in_domain(vec![]);
        let baseline = baseline_of(&[frozen_edge(&rule.name, "domain::service", "infra::db")]);
        let result = check_rule_with_baseline(&service_to_db_graph(), &rule, false, &baseline);
        assert!(result.reported().next().is_none());
        assert_eq!(result.frozen().count(), 1);
        assert!(!result.has_negative_judgment());
    }

    #[test]
    fn test_a_frozen_edge_that_gains_a_symbol_is_reported_with_what_it_froze() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_named_dep(&mut graph, service, db, &["Pool", "Row"]);

        let rule = no_infra_in_domain(vec![]);
        let baseline = baseline_of(&[named_frozen_edge(
            &rule.name,
            "domain::service",
            "infra::db",
            &["Pool"],
        )]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);

        assert!(result.frozen().next().is_none());
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(reported.len(), 1);
        let ViolationDetail::Edge { frozen_for, .. } = &reported[0].detail else {
            panic!("expected an edge detail");
        };
        assert_eq!(frozen_for.as_ref().unwrap(), &named(&["Pool"]));
    }

    #[test]
    fn test_an_outgrown_entry_still_counts_as_a_hit() {
        // Otherwise the entry reads as freezing nothing, while it does match
        // its edge and only stopped covering it.
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_named_dep(&mut graph, service, db, &["Pool", "Row"]);

        let rule = no_infra_in_domain(vec![]);
        let baseline = baseline_of(&[named_frozen_edge(
            &rule.name,
            "domain::service",
            "infra::db",
            &["Pool"],
        )]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert_eq!(result.baseline_hits.len(), 1);
        assert_eq!(result.baseline_hits[0].key.symbols, named(&["Pool", "Row"]));
    }

    #[test]
    fn test_a_frozen_edge_stays_frozen_when_it_carries_less() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_named_dep(&mut graph, service, db, &["Pool"]);

        let rule = no_infra_in_domain(vec![]);
        let baseline = baseline_of(&[named_frozen_edge(
            &rule.name,
            "domain::service",
            "infra::db",
            &["Pool", "Row"],
        )]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert!(result.reported().next().is_none());
        assert_eq!(result.frozen().count(), 1);
    }

    #[test]
    fn test_baseline_entry_scoped_to_rule_name_does_not_cover_other_rule() {
        let rule = no_infra_in_domain(vec![]);
        let baseline = baseline_of(&[frozen_edge(
            "some other rule",
            "domain::service",
            "infra::db",
        )]);
        let result = check_rule_with_baseline(&service_to_db_graph(), &rule, false, &baseline);
        assert_eq!(result.reported().count(), 1);
        assert!(result.frozen().next().is_none());
    }

    #[test]
    fn test_a_wholly_frozen_tangle_is_one_frozen_cluster() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let c = add_module(&mut graph, "c", crate_idx, crate_idx);
        let d = add_module(&mut graph, "d", crate_idx, crate_idx);
        // Two triangles sharing edge a -> b: one cluster, two rings.
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, c);
        add_production_dep(&mut graph, c, a);
        add_production_dep(&mut graph, b, d);
        add_production_dep(&mut graph, d, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let baseline = baseline_of(&[
            frozen_edge(&rule.name, "test::a", "test::b"),
            frozen_edge(&rule.name, "test::b", "test::c"),
            frozen_edge(&rule.name, "test::c", "test::a"),
            frozen_edge(&rule.name, "test::b", "test::d"),
            frozen_edge(&rule.name, "test::d", "test::a"),
        ]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert_eq!(result.reported().count(), 0);
        assert_eq!(
            result.frozen().count(),
            1,
            "one frozen cluster, not one per frozen edge"
        );
        let ViolationDetail::Cluster(cluster) = &result.frozen().next().unwrap().detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 2);
        assert!(!cluster.feedback_edges.is_empty());
        assert!(!result.has_negative_judgment());
    }

    #[test]
    fn test_cycle_sharing_an_edge_with_a_frozen_cycle_is_still_reported() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let c = add_module(&mut graph, "c", crate_idx, crate_idx);
        let d = add_module(&mut graph, "d", crate_idx, crate_idx);
        // a -> b -> c -> a and a -> b -> d -> a share edge a -> b.
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, c);
        add_production_dep(&mut graph, c, a);
        add_production_dep(&mut graph, b, d);
        add_production_dep(&mut graph, d, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let baseline = baseline_of(&[
            frozen_edge(&rule.name, "test::a", "test::b"),
            frozen_edge(&rule.name, "test::b", "test::c"),
            frozen_edge(&rule.name, "test::c", "test::a"),
        ]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert_eq!(
            result.frozen().count(),
            0,
            "the cluster is mixed, so it is reported; a mixed cluster has no frozen twin"
        );
        let reported: Vec<_> = result.reported().collect();
        assert_eq!(
            reported.len(),
            1,
            "the a-b-d cycle shares edge a->b with the frozen cycle, but is a distinct \
             violation and must still be reported"
        );
        let ViolationDetail::Cluster(cluster) = &reported[0].detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 1);
        let cycle = cluster.cycle.as_ref().expect("single remaining cycle");
        assert!(cycle.iter().any(|m| m == "d"), "got: {cycle:?}");
        assert!(!cycle.iter().any(|m| m == "c"), "got: {cycle:?}");
    }

    #[test]
    fn test_baseline_generated_from_a_run_covers_the_next_run() {
        let (mut graph, _domain, service, _model, _infra, db, api, _app, _handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, service, db);
        add_production_dep(&mut graph, db, api);
        add_production_dep(&mut graph, api, db);

        let config = config_of(vec![
            no_infra_in_domain(vec![]),
            no_cycles_rule("no cycles in infra", "infra::**", vec![]),
        ]);

        let first = check_rules(&graph, &config, &Baseline::empty(), false).unwrap();
        assert!(
            first.reported().next().is_some(),
            "sanity: violations exist"
        );

        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, &first.baseline_entries).unwrap();
        let baseline = Baseline::load(&path).unwrap();

        let second = check_rules(&graph, &config, &baseline, false).unwrap();
        let reported: Vec<_> = second.reported().collect();
        assert!(reported.is_empty(), "got: {reported:?}");
        assert!(second.frozen().next().is_some());
    }
}
