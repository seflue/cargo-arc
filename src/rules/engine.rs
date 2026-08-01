//! Rule evaluation engine
//!
//! Checks architecture rules against the dependency graph and collects violations.

use crate::diagnose::{Cluster, CycleAnalysis, MinimalCycles};
use crate::graph::{ArcGraph, Edge};
use crate::model::SourceLocation;
use crate::rules::config::{ArcConfig, Direction, Rule, Severity};
use crate::rules::matching::resolve_pattern;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;

/// A single architecture rule violation.
#[derive(Debug)]
pub struct Violation {
    pub rule_name: String,
    pub rule_type: String,
    pub severity: Severity,
    pub detail: ViolationDetail,
    pub locations: Vec<SourceLocation>,
}

/// Violation payload: either a rendered message, or structured data the
/// renderer branches on.
#[derive(Debug)]
pub enum ViolationDetail {
    /// Rendered text. forbidden-dependency and layers still build their message
    /// in the checker; splitting those into fields is a separate change.
    Message(String),
    Cluster(CycleCluster),
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
#[derive(Debug)]
pub struct CheckResult {
    pub violations: Vec<Violation>,
}

impl CheckResult {
    /// Whether any violation has `Severity::Error`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.violations
            .iter()
            .any(|v| v.severity == Severity::Error)
    }

    /// Exit code: 1 if errors exist, 0 otherwise.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        i32::from(self.has_errors())
    }
}

/// Check all rules in the config against the graph.
///
/// Dispatches each rule to its type-specific checker, collects all violations,
/// and filters out `Severity::Ignore` rules.
#[must_use]
pub fn check_rules(graph: &ArcGraph, config: &ArcConfig, include_reexports: bool) -> CheckResult {
    let violations = config
        .rules
        .iter()
        .filter(|rule| !matches!(rule_severity(rule), Severity::Ignore))
        .flat_map(|rule| match rule {
            Rule::ForbiddenDependency { .. } => check_forbidden(graph, rule),
            Rule::NoCycles { .. } => check_cycles(graph, rule, include_reexports),
            Rule::Layers { .. } => check_layers(graph, rule),
        })
        .collect();
    CheckResult { violations }
}

fn rule_severity(rule: &Rule) -> Severity {
    match rule {
        Rule::ForbiddenDependency { severity, .. }
        | Rule::NoCycles { severity, .. }
        | Rule::Layers { severity, .. } => *severity,
    }
}

/// Check a `forbidden-dependency` rule: any production edge from `from` nodes
/// to `to` nodes is a violation.
fn check_forbidden(graph: &ArcGraph, rule: &Rule) -> Vec<Violation> {
    let Rule::ForbiddenDependency {
        name,
        from,
        to,
        severity,
    } = rule
    else {
        return Vec::new();
    };

    let from_set: HashSet<NodeIndex> = resolve_pattern(from, graph).into_iter().collect();
    let to_set: HashSet<NodeIndex> = resolve_pattern(to, graph).into_iter().collect();

    graph
        .edge_indices()
        .filter_map(|edge_idx| {
            let edge = &graph[edge_idx];
            if !edge.is_production() {
                return None;
            }
            let (source, target) = graph.edge_endpoints(edge_idx).expect("edge should exist");
            if !from_set.contains(&source) || !to_set.contains(&target) {
                return None;
            }
            let source_path = graph.qualified_name(source);
            let target_path = graph.qualified_name(target);
            let locations = match edge {
                Edge::ModuleDep { locations, .. } => locations.clone(),
                _ => Vec::new(),
            };
            Some(Violation {
                rule_name: name.clone(),
                rule_type: "forbidden-dependency".into(),
                severity: *severity,
                detail: ViolationDetail::Message(format!("{source_path} → {target_path}")),
                locations,
            })
        })
        .collect()
}

/// Check a `no-cycles` rule: find elementary cycles within the scoped subgraph.
/// Pure re-export cycles are excluded unless `include_reexports` is set (ADR-022).
fn check_cycles(graph: &ArcGraph, rule: &Rule, include_reexports: bool) -> Vec<Violation> {
    let Rule::NoCycles {
        name,
        scope,
        severity,
    } = rule
    else {
        return Vec::new();
    };

    let scope_set: HashSet<NodeIndex> = resolve_pattern(scope, graph).into_iter().collect();

    // Build a subgraph with only production module-dep edges between scope nodes.
    // Pure re-export edges are excluded by default (ADR-022): idiomatic
    // republishing is not a real cycle unless --include-reexports asks for it.
    let subgraph = graph.filter_map(
        |idx, _| scope_set.contains(&idx).then_some(idx),
        |_, edge| {
            (edge.is_production_module_dep()
                && (include_reexports || !edge.is_reexport_module_dep()))
            .then_some(())
        },
    );

    let analysis = subgraph.minimal_cycles();
    // The cluster report is computed per rule, over that rule's own scoped
    // subgraph: two no-cycles rules with different scopes see different views.
    let report = graph.cluster_report(&subgraph, &analysis);
    let total = report.clusters.len();

    report
        .clusters
        .iter()
        .enumerate()
        .map(|(i, cluster)| Violation {
            rule_name: name.clone(),
            rule_type: "no-cycles".into(),
            severity: *severity,
            detail: ViolationDetail::Cluster(CycleCluster::from_cluster(
                graph,
                &analysis,
                cluster,
                i + 1,
                total,
            )),
            locations: Vec::new(),
        })
        .collect()
}

/// Check a `layers` rule: edges must respect layer ordering.
fn check_layers(graph: &ArcGraph, rule: &Rule) -> Vec<Violation> {
    let Rule::Layers {
        name,
        layers,
        direction,
        severity,
    } = rule
    else {
        return Vec::new();
    };

    // Build layer index: NodeIndex → layer position
    let mut layer_index: std::collections::HashMap<NodeIndex, usize> =
        std::collections::HashMap::new();
    for (pos, layer_pattern) in layers.iter().enumerate() {
        for idx in resolve_pattern(layer_pattern, graph) {
            layer_index.insert(idx, pos);
        }
    }

    graph
        .edge_indices()
        .filter_map(|edge_idx| {
            let edge = &graph[edge_idx];
            if !edge.is_production() {
                return None;
            }
            let (source, target) = graph.edge_endpoints(edge_idx).expect("edge should exist");
            let source_layer = layer_index.get(&source)?;
            let target_layer = layer_index.get(&target)?;

            let violation = match direction {
                // top-down: higher layers (lower index) may depend on lower layers (higher index)
                Direction::TopDown => source_layer > target_layer,
                // bottom-up: lower layers may depend on higher layers
                Direction::BottomUp => source_layer < target_layer,
            };

            if !violation {
                return None;
            }

            let source_path = graph.qualified_name(source);
            let target_path = graph.qualified_name(target);
            let locations = match edge {
                Edge::ModuleDep { locations, .. } => locations.clone(),
                _ => Vec::new(),
            };
            Some(Violation {
                rule_name: name.clone(),
                rule_type: "layers".into(),
                severity: *severity,
                detail: ViolationDetail::Message(format!("{source_path} → {target_path}")),
                locations,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;
    use crate::model::EdgeContext;
    use std::path::PathBuf;

    // -- Test graph helpers --

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

        let rule = Rule::ForbiddenDependency {
            name: "no infra in domain".into(),
            from: "domain::**".into(),
            to: "infra::**".into(),
            severity: Severity::Error,
        };
        let violations = check_forbidden(&graph, &rule);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_name, "no infra in domain");
        assert_eq!(violations[0].rule_type, "forbidden-dependency");
        let ViolationDetail::Message(msg) = &violations[0].detail else {
            panic!("expected a message detail");
        };
        assert!(msg.contains("service"));
        assert!(msg.contains("db"));
    }

    #[test]
    fn test_forbidden_no_violation() {
        let (mut graph, _domain, service, _model, _infra, _db, _api, _application, handler) =
            multi_crate_graph();
        // domain::service → application::handler (allowed, rule forbids domain→infra)
        add_production_dep(&mut graph, service, handler);

        let rule = Rule::ForbiddenDependency {
            name: "no infra in domain".into(),
            from: "domain::**".into(),
            to: "infra::**".into(),
            severity: Severity::Error,
        };
        let violations = check_forbidden(&graph, &rule);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_forbidden_multiple_violations() {
        let (mut graph, _domain, service, model, _infra, db, api, _app, _handler) =
            multi_crate_graph();
        // Two forbidden edges: service→db and model→api
        add_production_dep(&mut graph, service, db);
        add_production_dep(&mut graph, model, api);

        let rule = Rule::ForbiddenDependency {
            name: "no infra in domain".into(),
            from: "domain::**".into(),
            to: "infra::**".into(),
            severity: Severity::Error,
        };
        let violations = check_forbidden(&graph, &rule);
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn test_forbidden_ignores_test_edges() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // Test-only edge: should not trigger violation
        add_test_dep(&mut graph, service, db);

        let rule = Rule::ForbiddenDependency {
            name: "no infra in domain".into(),
            from: "domain::**".into(),
            to: "infra::**".into(),
            severity: Severity::Error,
        };
        let violations = check_forbidden(&graph, &rule);
        assert!(violations.is_empty());
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

        let rule = Rule::NoCycles {
            name: "no cycles in test".into(),
            scope: "test::**".into(),
            severity: Severity::Error,
        };
        let violations = check_cycles(&graph, &rule, false);
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

        let rule = Rule::NoCycles {
            name: "no cycles".into(),
            scope: "test::**".into(),
            severity: Severity::Error,
        };
        // Default: the idiomatic re-export cycle is not reported (ADR-022).
        assert!(
            check_cycles(&graph, &rule, false).is_empty(),
            "pure re-export cycle should be ignored by default"
        );
        // --include-reexports opts back into the full graph and surfaces it.
        assert_eq!(
            check_cycles(&graph, &rule, true).len(),
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
        let rule = Rule::NoCycles {
            name: "no cycles in domain".into(),
            scope: "domain::**".into(),
            severity: Severity::Error,
        };
        let violations = check_cycles(&graph, &rule, false);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_no_cycles() {
        let (mut graph, _domain, service, model, _infra, _db, _api, _app, _handler) =
            multi_crate_graph();
        // Linear: service → model (no cycle)
        add_production_dep(&mut graph, service, model);

        let rule = Rule::NoCycles {
            name: "no cycles in domain".into(),
            scope: "domain::**".into(),
            severity: Severity::Error,
        };
        let violations = check_cycles(&graph, &rule, false);
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

        let rule = Rule::NoCycles {
            name: "global no-cycles".into(),
            scope: "**".into(),
            severity: Severity::Error,
        };
        let violations = check_cycles(&graph, &rule, false);
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

        let rule = Rule::NoCycles {
            name: "no cycles in test".into(),
            scope: "test::**".into(),
            severity: Severity::Error,
        };
        let violations = check_cycles(&graph, &rule, false);
        assert_eq!(violations.len(), 1);
        let ViolationDetail::Cluster(cluster) = &violations[0].detail else {
            panic!("expected a cluster detail");
        };
        assert_eq!(cluster.cycles, 2);
        assert!(cluster.ring.is_none());
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

        let rule = Rule::Layers {
            name: "architecture layers".into(),
            layers: vec!["domain".into(), "application".into(), "infra".into()],
            direction: Direction::TopDown,
            severity: Severity::Error,
        };
        let violations = check_layers(&graph, &rule);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_layers_violation_bottom_up() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // infra::db → domain::service (bottom-up in top-down rule = violation)
        add_production_dep(&mut graph, db, service);

        let rule = Rule::Layers {
            name: "architecture layers".into(),
            layers: vec!["domain".into(), "application".into(), "infra".into()],
            direction: Direction::TopDown,
            severity: Severity::Error,
        };
        let violations = check_layers(&graph, &rule);
        assert_eq!(violations.len(), 1);
        let ViolationDetail::Message(msg) = &violations[0].detail else {
            panic!("expected a message detail");
        };
        assert!(msg.contains("db"));
        assert!(msg.contains("service"));
    }

    #[test]
    fn test_layers_skip_layer() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        // domain::service → infra::db (skipping application layer — allowed in top-down)
        add_production_dep(&mut graph, service, db);

        let rule = Rule::Layers {
            name: "architecture layers".into(),
            layers: vec!["domain".into(), "application".into(), "infra".into()],
            direction: Direction::TopDown,
            severity: Severity::Error,
        };
        let violations = check_layers(&graph, &rule);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_layers_outside_layers() {
        let (mut graph, crate_idx) = test_crate_graph();
        let a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // Edge between modules not in any defined layer — should be ignored
        add_production_dep(&mut graph, a, b);

        let rule = Rule::Layers {
            name: "architecture layers".into(),
            layers: vec!["domain".into(), "infra".into()],
            direction: Direction::TopDown,
            severity: Severity::Error,
        };
        let violations = check_layers(&graph, &rule);
        assert!(violations.is_empty());
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

        let config = ArcConfig {
            config: None,
            rules: vec![
                Rule::ForbiddenDependency {
                    name: "no infra in domain".into(),
                    from: "domain::**".into(),
                    to: "infra::**".into(),
                    severity: Severity::Error,
                },
                Rule::NoCycles {
                    name: "no cycles in infra".into(),
                    scope: "infra::**".into(),
                    severity: Severity::Warn,
                },
            ],
        };
        let result = check_rules(&graph, &config, false);
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
        let config = ArcConfig {
            config: None,
            rules: vec![],
        };
        let result = check_rules(&graph, &config, false);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn test_check_result_has_errors() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                detail: ViolationDetail::Message("a → b".into()),
                locations: vec![],
            }],
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
                detail: ViolationDetail::Message("cycle".into()),
                locations: vec![],
            }],
        };
        assert!(!result.has_errors());
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn test_severity_ignore_filtered() {
        let (mut graph, _domain, service, _model, _infra, db, _api, _app, _handler) =
            multi_crate_graph();
        add_production_dep(&mut graph, service, db);

        let config = ArcConfig {
            config: None,
            rules: vec![Rule::ForbiddenDependency {
                name: "ignored rule".into(),
                from: "domain::**".into(),
                to: "infra::**".into(),
                severity: Severity::Ignore,
            }],
        };
        let result = check_rules(&graph, &config, false);
        assert!(result.violations.is_empty());
    }
}
