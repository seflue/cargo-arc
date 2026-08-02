//! Rule evaluation engine
//!
//! Checks architecture rules against the dependency graph and collects violations.

use crate::diagnose::{Cluster, CycleAnalysis, MinimalCycles};
use crate::graph::{ArcGraph, Edge};
use crate::model::SourceLocation;
use crate::rules::baseline::{Baseline, BaselineEntry, FindingKey};
use crate::rules::config::{
    ArcConfig, DiagnosticLevel, Direction, Except, ForbiddenDependencyRule, LayersRule,
    NoCyclesRule, Rule, RuleKind, Severity,
};
use crate::rules::diagnostics::{self, Diagnostic};
use crate::rules::matching::PatternIndex;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{HashMap, HashSet};

/// A single architecture rule violation.
#[derive(Debug)]
pub struct Violation {
    pub rule_name: String,
    pub rule_type: String,
    pub severity: Severity,
    pub detail: ViolationDetail,
    pub locations: Vec<SourceLocation>,
}

/// Violation payload: structured data the renderer branches on.
#[derive(Debug)]
pub enum ViolationDetail {
    /// A single forbidden edge, source and target as fully qualified names.
    /// forbidden-dependency and layers both report this shape.
    Edge {
        from: String,
        to: String,
    },
    Cluster(CycleCluster),
    /// A single ring the baseline keeps out of `violations`; carries the
    /// ring's members since a cluster is not a stable object to key on.
    Ring {
        modules: Vec<String>,
    },
}

/// One cyclic cluster, resolved to names and counts for rendering.
#[derive(Debug)]
pub struct CycleCluster {
    /// 1-based position among the clusters of the same rule.
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
    /// Crate-relative ring names, set when the cluster holds exactly one cycle.
    pub ring: Option<Vec<String>>,
    /// Feedback edges, crate-relative names, ranked as `Cluster::feedback_edges`.
    pub feedback_edges: Vec<CycleClusterEdge>,
}

/// One feedback edge of a [`CycleCluster`], names already resolved.
#[derive(Debug)]
pub struct CycleClusterEdge {
    pub from: String,
    pub to: String,
    pub cycles: usize,
    pub refs: usize,
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
        let ring = (cluster.cycles.len() == 1).then(|| {
            analysis.cycles[cluster.cycles[0]]
                .path
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
                refs: edge.refs,
            })
            .collect();
        Self {
            position,
            total,
            crate_name,
            place,
            modules: cluster.nodes.len(),
            cycles: cluster.cycles.len(),
            ring,
            feedback_edges,
        }
    }
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
    pub violations: Vec<Violation>,
    /// Violations suppressed by an `except` entry: they never affect
    /// `has_errors`/`exit_code`, and are only printed under
    /// `--show-suppressed`.
    pub suppressed: Vec<Violation>,
    /// Findings an `arc-baseline.toml` entry covers: like `suppressed`, they
    /// never affect `has_errors`/`exit_code`.
    pub baselined: Vec<Violation>,
    /// The key of every finding actually reported in `violations`, i.e. what
    /// `--generate-baseline` writes out.
    pub baseline_entries: Vec<BaselineEntry>,
    /// The baseline entries this run matched. What is left of the baseline
    /// beyond them no longer suppresses anything.
    pub baseline_hits: Vec<BaselineEntry>,
    /// Gaps in the configuration, unrelated to any single rule.
    pub diagnostics: Vec<Diagnostic>,
}

impl CheckResult {
    /// Whether any violation has `Severity::Error`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.violations
            .iter()
            .any(|v| v.severity == Severity::Error)
    }

    /// Exit code: 1 if a rule was violated at error level or a diagnostic is
    /// set to `deny`, 0 otherwise.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        let denied = self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Deny);
        i32::from(self.has_errors() || denied)
    }
}

impl FromIterator<CheckResult> for CheckResult {
    fn from_iter<I: IntoIterator<Item = CheckResult>>(iter: I) -> Self {
        iter.into_iter().fold(Self::default(), |mut acc, result| {
            acc.violations.extend(result.violations);
            acc.suppressed.extend(result.suppressed);
            acc.baselined.extend(result.baselined);
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
    fn check_rule(&self, rule: &Rule) -> CheckResult {
        match &rule.kind {
            RuleKind::ForbiddenDependency(params) => self.check_forbidden(rule, params),
            RuleKind::NoCycles(params) => self.check_cycles(rule, params),
            RuleKind::Layers(params) => self.check_layers(rule, params),
        }
    }

    /// Check a `forbidden-dependency` rule: any production edge from `from`
    /// nodes to `to` nodes is a violation.
    fn check_forbidden(&self, rule: &Rule, params: &ForbiddenDependencyRule) -> CheckResult {
        let from_set = self.resolve_set(&params.from);
        let to_set = self.resolve_set(&params.to);
        let except = ResolvedExceptions::resolve(&rule.except, self);

        self.check_edge_violations(rule, &except, |source, target| {
            from_set.contains(&source) && to_set.contains(&target)
        })
    }

    /// Check a `no-cycles` rule: find elementary cycles within the scoped
    /// subgraph. Pure re-export cycles are excluded unless `include_reexports`
    /// is set (ADR-022). An edge covered by `except` is removed before the
    /// search, so a ring built through it never forms; if it lay on one, it is
    /// reported as suppressed instead. The suppressed side holds removed
    /// edges, not rings: the two sides can have different SCC decompositions,
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

        let suppressed = drop_excepted_edges(&mut subgraph, excepted)
            .into_iter()
            .map(|(source, target)| Violation {
                rule_name: rule.name.clone(),
                rule_type: rule.rule_type().into(),
                severity: rule.severity(),
                detail: ViolationDetail::Edge {
                    from: graph.qualified_name(source),
                    to: graph.qualified_name(target),
                },
                locations: module_dep_locations(graph, source, target),
            })
            .collect();

        let mut analysis = subgraph.minimal_cycles();
        // The baseline keys on the ring, not the cluster: a cluster merges or
        // splits as edges are added, so it has no identity to freeze against.
        let mut baselined = Vec::new();
        let mut baseline_entries = Vec::new();
        let mut baseline_hits = Vec::new();
        analysis.retain_cycles(|cycle| {
            let modules: Vec<String> = cycle
                .path
                .iter()
                .map(|&idx| graph.qualified_name(idx))
                .collect();
            let key = FindingKey::ring(modules.clone());
            if self.baseline.covers(&rule.name, &key) {
                baseline_hits.push(BaselineEntry {
                    rule: rule.name.clone(),
                    key,
                });
                baselined.push(Violation {
                    rule_name: rule.name.clone(),
                    rule_type: rule.rule_type().into(),
                    severity: rule.severity(),
                    detail: ViolationDetail::Ring { modules },
                    locations: Vec::new(),
                });
                false
            } else {
                baseline_entries.push(BaselineEntry {
                    rule: rule.name.clone(),
                    key,
                });
                true
            }
        });

        // The cluster report is computed per rule, over that rule's own scoped
        // subgraph: two no-cycles rules with different scopes see different views.
        // Clusters left without a surviving ring drop out here on their own.
        let report = graph.cluster_report(&subgraph, &analysis);
        let total = report.clusters.len();

        let violations = report
            .clusters
            .iter()
            .enumerate()
            .map(|(i, cluster)| Violation {
                rule_name: rule.name.clone(),
                rule_type: rule.rule_type().into(),
                severity: rule.severity(),
                detail: ViolationDetail::Cluster(CycleCluster::from_cluster(
                    graph,
                    &analysis,
                    cluster,
                    i + 1,
                    total,
                )),
                locations: Vec::new(),
            })
            .collect();

        CheckResult {
            violations,
            suppressed,
            baselined,
            baseline_entries,
            baseline_hits,
            diagnostics: Vec::new(),
        }
    }

    /// Check a `layers` rule: edges must respect layer ordering.
    fn check_layers(&self, rule: &Rule, params: &LayersRule) -> CheckResult {
        // Build layer index: NodeIndex → layer position
        let mut layer_index: std::collections::HashMap<NodeIndex, usize> =
            std::collections::HashMap::new();
        for (pos, layer_pattern) in params.layers.iter().enumerate() {
            for idx in self.resolve(layer_pattern) {
                layer_index.insert(idx, pos);
            }
        }
        let except = ResolvedExceptions::resolve(&rule.except, self);

        self.check_edge_violations(rule, &except, |source, target| {
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
        })
    }

    /// Shared by all edge-predicate rule checks (forbidden-dependency, layers);
    /// only the predicate and the rule-type label differ between them. An edge
    /// covered by `except` still produces a `Violation`, but lands in the
    /// suppressed side. A baseline check runs only after that: `except` is a
    /// permanent allowance, the baseline a frozen one.
    fn check_edge_violations(
        &self,
        rule: &Rule,
        except: &ResolvedExceptions,
        is_violation: impl Fn(NodeIndex, NodeIndex) -> bool,
    ) -> CheckResult {
        let graph = self.graph();
        let mut violations = Vec::new();
        let mut suppressed = Vec::new();
        let mut baselined = Vec::new();
        let mut baseline_entries = Vec::new();
        let mut baseline_hits = Vec::new();
        for edge_idx in graph.edge_indices() {
            let edge = &graph[edge_idx];
            if !edge.is_production() {
                continue;
            }
            let (source, target) = graph.edge_endpoints(edge_idx).expect("edge should exist");
            if !is_violation(source, target) {
                continue;
            }
            let locations = match edge {
                Edge::ModuleDep { locations, .. } => locations.clone(),
                _ => Vec::new(),
            };
            let from = graph.qualified_name(source);
            let to = graph.qualified_name(target);
            let violation = Violation {
                rule_name: rule.name.clone(),
                rule_type: rule.rule_type().into(),
                severity: rule.severity(),
                detail: ViolationDetail::Edge {
                    from: from.clone(),
                    to: to.clone(),
                },
                locations,
            };
            if except.covers(source, target) {
                suppressed.push(violation);
                continue;
            }
            let key = FindingKey::edge(from, to);
            if self.baseline.covers(&rule.name, &key) {
                baseline_hits.push(BaselineEntry {
                    rule: rule.name.clone(),
                    key,
                });
                baselined.push(violation);
            } else {
                baseline_entries.push(BaselineEntry {
                    rule: rule.name.clone(),
                    key,
                });
                violations.push(violation);
            }
        }
        CheckResult {
            violations,
            suppressed,
            baselined,
            baseline_entries,
            baseline_hits,
            diagnostics: Vec::new(),
        }
    }

    /// Check all rules in `config` against the run.
    ///
    /// Diagnostics are raised after the rules: a stale baseline entry is only
    /// recognizable once every rule has had its chance to match it.
    #[must_use]
    pub(crate) fn check_all(&self, config: &ArcConfig) -> CheckResult {
        let mut result: CheckResult = config
            .rules
            .iter()
            .filter(|rule| rule.severity() != Severity::Ignore)
            .map(|rule| self.check_rule(rule))
            .collect();
        result.diagnostics = diagnostics::collect(
            &self.pattern_index,
            config,
            self.baseline,
            &result.baseline_hits,
        );
        result
    }

    #[must_use]
    pub(crate) fn dead_excepts(&self, config: &ArcConfig) -> Vec<diagnostics::DeadExcept> {
        diagnostics::dead_excepts(&self.pattern_index, config)
    }
}

/// Set up a run over `graph` and check every rule in `config` against it.
#[must_use]
pub fn check_rules(
    graph: &ArcGraph,
    config: &ArcConfig,
    baseline: &Baseline,
    include_reexports: bool,
) -> CheckResult {
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
            Edge::ModuleDep { locations, .. } => Some(locations.clone()),
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
    use crate::rules::diagnostics::DiagnosticKind;
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
        CheckRun::new(graph, baseline, include_reexports).check_rule(rule)
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
        graph.add_edge(parent, idx, Edge::Contains);
        idx
    }

    fn add_production_dep(graph: &mut ArcGraph, from: NodeIndex, to: NodeIndex) {
        graph.add_edge(
            from,
            to,
            Edge::ModuleDep {
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

    fn add_reexport_dep(graph: &mut ArcGraph, from: NodeIndex, to: NodeIndex) {
        graph.add_edge(
            from,
            to,
            Edge::ModuleDep {
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
            Edge::ModuleDep {
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

        let violations = check_rule(&graph, &no_infra_in_domain(vec![]), false).violations;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_name, "no infra in domain");
        assert_eq!(violations[0].rule_type, "forbidden-dependency");
        let ViolationDetail::Edge { from, to } = &violations[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(from.contains("service"));
        assert!(to.contains("db"));
    }

    #[test]
    fn test_forbidden_no_violation() {
        let (mut graph, _domain, service, _model, _infra, _db, _api, _application, handler) =
            multi_crate_graph();
        // domain::service → application::handler (allowed, rule forbids domain→infra)
        add_production_dep(&mut graph, service, handler);

        let violations = check_rule(&graph, &no_infra_in_domain(vec![]), false).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn test_forbidden_multiple_violations() {
        let (mut graph, _domain, service, model, _infra, db, api, _app, _handler) =
            multi_crate_graph();
        // Two forbidden edges: service→db and model→api
        add_production_dep(&mut graph, service, db);
        add_production_dep(&mut graph, model, api);

        let violations = check_rule(&graph, &no_infra_in_domain(vec![]), false).violations;
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn test_forbidden_ignores_test_edges() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // Test-only edge: should not trigger violation
        add_test_dep(&mut graph, service, db);

        let violations = check_rule(&graph, &no_infra_in_domain(vec![]), false).violations;
        assert!(violations.is_empty());
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
            declared_severity: Some(Severity::Error),
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
            declared_severity: Some(Severity::Error),
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
            declared_severity: Some(Severity::Error),
            except,
            kind: RuleKind::Layers(LayersRule {
                layers: layers.iter().map(|&layer| layer.into()).collect(),
                direction: Direction::TopDown,
            }),
        }
    }

    /// `ArcConfig` over `rules`, without a `[config]` block and with the
    /// default diagnostic levels.
    fn config_of(rules: Vec<Rule>) -> ArcConfig {
        ArcConfig {
            config: None,
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
    fn test_forbidden_except_suppresses_matching_edge() {
        let rule = no_infra_in_domain(vec![except_edge("domain::service", "infra::db")]);
        let CheckResult {
            violations,
            suppressed,
            ..
        } = check_rule(&service_to_db_graph(), &rule, false);
        assert!(violations.is_empty());
        assert_eq!(suppressed.len(), 1);
        let ViolationDetail::Edge { from, to } = &suppressed[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(from.contains("service"));
        assert!(to.contains("db"));
    }

    #[test]
    fn test_forbidden_except_not_matching_leaves_violation_reported() {
        let rule = no_infra_in_domain(vec![except_edge("domain::model", "infra::db")]);
        let CheckResult {
            violations,
            suppressed,
            ..
        } = check_rule(&service_to_db_graph(), &rule, false);
        assert_eq!(violations.len(), 1);
        assert!(suppressed.is_empty());
    }

    #[test]
    fn test_forbidden_except_pattern_matches_glob() {
        let rule = no_infra_in_domain(vec![except_edge("domain::**", "infra::**")]);
        let CheckResult {
            violations,
            suppressed,
            ..
        } = check_rule(&service_to_db_graph(), &rule, false);
        assert!(violations.is_empty());
        assert_eq!(suppressed.len(), 1);
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
        let violations = check_rule(&graph, &rule, false).violations;
        assert_eq!(violations.len(), 1);
        let ViolationDetail::Cluster(cluster) = &violations[0].detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 1);
        assert!(cluster.ring.is_some());
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
            check_rule(&graph, &rule, false).violations.is_empty(),
            "pure re-export cycle should be ignored by default"
        );
        // --include-reexports opts back into the full graph and surfaces it.
        assert_eq!(
            check_rule(&graph, &rule, true).violations.len(),
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
        let violations = check_rule(&graph, &rule, false).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn test_no_cycles() {
        let (mut graph, _domain, service, model, _infra, _db, _api, _app, _handler) =
            multi_crate_graph();
        // Linear: service → model (no cycle)
        add_production_dep(&mut graph, service, model);

        let rule = no_cycles_rule("no cycles in domain", "domain::**", vec![]);
        let violations = check_rule(&graph, &rule, false).violations;
        assert!(violations.is_empty());
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
        let violations = check_rule(&graph, &rule, false).violations;
        assert_eq!(violations.len(), 2);
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
        let violations = check_rule(&graph, &rule, false).violations;
        assert_eq!(violations.len(), 1);
        let ViolationDetail::Cluster(cluster) = &violations[0].detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 2);
        assert!(cluster.ring.is_none());
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
        let CheckResult {
            violations,
            suppressed,
            ..
        } = check_rule(&graph, &rule, false);
        assert!(
            violations.is_empty(),
            "except should remove the edge before the ring can form"
        );
        assert_eq!(suppressed.len(), 1);
        let ViolationDetail::Edge { from, to } = &suppressed[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(from.contains('b'));
        assert!(to.contains('a'));
        assert_eq!(
            suppressed[0].locations.len(),
            1,
            "the excepted edge's source locations belong on the suppressed finding"
        );
    }

    #[test]
    fn test_cycles_except_on_edge_off_any_ring_is_not_recorded() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // a → b is the only edge: nothing here ever forms a ring.
        add_production_dep(&mut graph, a, b);

        let rule = no_cycles_rule(
            "no cycles in test",
            "test::**",
            vec![except_edge("test::a", "test::b")],
        );
        let CheckResult {
            violations,
            suppressed,
            ..
        } = check_rule(&graph, &rule, false);
        assert!(violations.is_empty());
        assert!(
            suppressed.is_empty(),
            "an edge that never lay on a ring is not a suppressed finding"
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
        let violations = check_rule(&graph, &rule, false).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn test_layers_violation_bottom_up() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // infra::db → domain::service (bottom-up in top-down rule = violation)
        add_production_dep(&mut graph, db, service);

        let rule = layers_rule(&["domain", "application", "infra"], vec![]);
        let violations = check_rule(&graph, &rule, false).violations;
        assert_eq!(violations.len(), 1);
        let ViolationDetail::Edge { from, to } = &violations[0].detail else {
            panic!("expected an edge detail");
        };
        assert!(from.contains("db"));
        assert!(to.contains("service"));
    }

    #[test]
    fn test_layers_skip_layer() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // domain::service → infra::db (skipping application layer — allowed in top-down)
        add_production_dep(&mut graph, service, db);

        let rule = layers_rule(&["domain", "application", "infra"], vec![]);
        let violations = check_rule(&graph, &rule, false).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn test_layers_outside_layers() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // Edge between modules not in any defined layer — should be ignored
        add_production_dep(&mut graph, a, b);

        let rule = layers_rule(&["domain", "infra"], vec![]);
        let violations = check_rule(&graph, &rule, false).violations;
        assert!(violations.is_empty());
    }

    #[test]
    fn test_layers_except_suppresses_matching_edge() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // infra::db → domain::service (bottom-up in top-down rule), but excepted.
        add_production_dep(&mut graph, db, service);

        let rule = layers_rule(
            &["domain", "application", "infra"],
            vec![except_edge("infra::db", "domain::service")],
        );
        let CheckResult {
            violations,
            suppressed,
            ..
        } = check_rule(&graph, &rule, false);
        assert!(violations.is_empty());
        assert_eq!(suppressed.len(), 1);
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
                declared_severity: Some(Severity::Warn),
                except: vec![],
                kind: RuleKind::NoCycles(NoCyclesRule {
                    scope: "infra::**".into(),
                }),
            },
        ]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false);
        assert_eq!(result.violations.len(), 2);
        assert!(
            result
                .violations
                .iter()
                .any(|v| v.rule_type == "forbidden-dependency")
        );
        assert!(result.violations.iter().any(|v| v.rule_type == "no-cycles"));
    }

    #[test]
    fn test_check_rules_empty() {
        let (graph, _) = test_crate_graph();
        let config = config_of(vec![]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn test_check_result_has_errors() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                detail: ViolationDetail::Edge {
                    from: "a".into(),
                    to: "b".into(),
                },
                locations: vec![],
            }],
            ..Default::default()
        };
        assert!(result.has_errors());
        assert_eq!(result.exit_code(), 1);
    }

    #[test]
    fn test_check_result_only_warnings() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "no-cycles".into(),
                severity: Severity::Warn,
                detail: ViolationDetail::Edge {
                    from: "a".into(),
                    to: "b".into(),
                },
                locations: vec![],
            }],
            ..Default::default()
        };
        assert!(!result.has_errors());
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_check_result_exit_code_ignores_suppressed_error() {
        let result = CheckResult {
            suppressed: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                detail: ViolationDetail::Edge {
                    from: "a".into(),
                    to: "b".into(),
                },
                locations: vec![],
            }],
            ..Default::default()
        };
        assert!(!result.has_errors());
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_check_rules_reports_a_crate_outside_every_layer() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, service, db);

        // The layers rule names two of the three crates.
        let config = config_of(vec![layers_rule(&["infra", "domain"], vec![])]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false);
        let unlayered: Vec<&str> = result
            .diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::UnlayeredCrate { krate } => Some(krate.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(unlayered, ["application"]);
    }

    #[test]
    fn test_a_baselined_finding_counts_as_a_baseline_hit() {
        let rule = no_infra_in_domain(vec![]);
        let entry = BaselineEntry {
            rule: rule.name.clone(),
            key: FindingKey::edge("domain::service", "infra::db"),
        };
        let baseline = baseline_of(std::slice::from_ref(&entry));
        let result = check_rule_with_baseline(&service_to_db_graph(), &rule, false, &baseline);
        assert_eq!(result.baseline_hits.len(), 1);
        assert_eq!(result.baseline_hits[0].rule, entry.rule);
        assert_eq!(result.baseline_hits[0].key, entry.key);
    }

    #[test]
    fn test_a_baselined_ring_counts_as_a_baseline_hit() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        let key = FindingKey::ring(vec!["test::a".to_string(), "test::b".to_string()]);
        let baseline = baseline_of(&[BaselineEntry {
            rule: rule.name.clone(),
            key: key.clone(),
        }]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert_eq!(result.baseline_hits.len(), 1);
        assert_eq!(result.baseline_hits[0].key, key);
    }

    #[test]
    fn test_a_denied_diagnostic_fails_the_run() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Deny,
                kind: DiagnosticKind::UnlayeredCrate {
                    krate: "xtask".into(),
                },
            }],
            ..Default::default()
        };
        assert!(!result.has_errors(), "no rule was violated");
        assert_eq!(result.exit_code(), 1);
    }

    #[test]
    fn test_a_warned_diagnostic_leaves_the_run_green() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warn,
                kind: DiagnosticKind::UnlayeredCrate {
                    krate: "xtask".into(),
                },
            }],
            ..Default::default()
        };
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_severity_ignore_filtered() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, service, db);

        let config = config_of(vec![Rule {
            name: "ignored rule".into(),
            declared_severity: Some(Severity::Ignore),
            except: vec![],
            kind: RuleKind::ForbiddenDependency(ForbiddenDependencyRule {
                from: "domain::**".into(),
                to: "infra::**".into(),
            }),
        }]);
        let result = check_rules(&graph, &config, &Baseline::empty(), false);
        assert!(result.violations.is_empty());
    }

    // ===== Baseline tests =====

    #[test]
    fn test_baselined_edge_is_not_a_violation() {
        let rule = no_infra_in_domain(vec![]);
        let baseline = baseline_of(&[BaselineEntry {
            rule: rule.name.clone(),
            key: FindingKey::edge("domain::service", "infra::db"),
        }]);
        let result = check_rule_with_baseline(&service_to_db_graph(), &rule, false, &baseline);
        assert!(result.violations.is_empty());
        assert_eq!(result.baselined.len(), 1);
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_baseline_entry_scoped_to_rule_name_does_not_cover_other_rule() {
        let rule = no_infra_in_domain(vec![]);
        let baseline = baseline_of(&[BaselineEntry {
            rule: "some other rule".into(),
            key: FindingKey::edge("domain::service", "infra::db"),
        }]);
        let result = check_rule_with_baseline(&service_to_db_graph(), &rule, false, &baseline);
        assert_eq!(result.violations.len(), 1);
        assert!(result.baselined.is_empty());
    }

    #[test]
    fn test_baselining_every_ring_of_a_cluster_removes_the_cluster() {
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
            BaselineEntry {
                rule: rule.name.clone(),
                key: FindingKey::ring(vec![
                    "test::a".to_string(),
                    "test::b".to_string(),
                    "test::c".to_string(),
                ]),
            },
            BaselineEntry {
                rule: rule.name.clone(),
                key: FindingKey::ring(vec![
                    "test::a".to_string(),
                    "test::b".to_string(),
                    "test::d".to_string(),
                ]),
            },
        ]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert!(result.violations.is_empty());
        assert_eq!(result.baselined.len(), 2);
    }

    #[test]
    fn test_ring_sharing_an_edge_with_a_baselined_ring_is_still_reported() {
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
        // Only the a-b-c ring is baselined.
        let baseline = baseline_of(&[BaselineEntry {
            rule: rule.name.clone(),
            key: FindingKey::ring(vec![
                "test::a".to_string(),
                "test::b".to_string(),
                "test::c".to_string(),
            ]),
        }]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert_eq!(result.baselined.len(), 1);
        assert_eq!(
            result.violations.len(),
            1,
            "the a-b-d ring shares edge a->b with the baselined ring, but is a distinct \
             finding and must still be reported"
        );
        let ViolationDetail::Cluster(cluster) = &result.violations[0].detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 1);
        let ring = cluster.ring.as_ref().expect("single remaining ring");
        assert!(ring.iter().any(|m| m == "d"), "got: {ring:?}");
        assert!(!ring.iter().any(|m| m == "c"), "got: {ring:?}");
    }

    #[test]
    fn test_rotated_baseline_entry_covers_the_same_ring() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let c = add_module(&mut graph, "c", crate_idx, crate_idx);
        add_production_dep(&mut graph, a, b);
        add_production_dep(&mut graph, b, c);
        add_production_dep(&mut graph, c, a);

        let rule = no_cycles_rule("no cycles in test", "test::**", vec![]);
        // Rotated relative to the traversal order (which starts at "test::a").
        let baseline = baseline_of(&[BaselineEntry {
            rule: rule.name.clone(),
            key: FindingKey::ring(vec![
                "test::c".to_string(),
                "test::a".to_string(),
                "test::b".to_string(),
            ]),
        }]);
        let result = check_rule_with_baseline(&graph, &rule, false, &baseline);
        assert!(result.violations.is_empty());
        assert_eq!(result.baselined.len(), 1);
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

        let first = check_rules(&graph, &config, &Baseline::empty(), false);
        assert!(!first.violations.is_empty(), "sanity: findings exist");

        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, &first.baseline_entries).unwrap();
        let baseline = Baseline::load(&path).unwrap();

        let second = check_rules(&graph, &config, &baseline, false);
        assert!(second.violations.is_empty(), "got: {:?}", second.violations);
        assert!(!second.baselined.is_empty());
    }
}
