//! Rule evaluation engine
//!
//! Checks architecture rules against the dependency graph and collects violations.

use crate::graph::{ArcGraph, Edge};
use crate::layout::ElementaryCycles;
use crate::model::SourceLocation;
use crate::rules::config::{ArcConfig, Direction, Rule, Severity};
use crate::rules::matching::{module_path, resolve_pattern};
use petgraph::graph::NodeIndex;
use std::collections::HashSet;

/// A single architecture rule violation.
#[derive(Debug)]
pub struct Violation {
    pub rule_name: String,
    pub rule_type: String,
    pub severity: Severity,
    pub message: String,
    pub locations: Vec<SourceLocation>,
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
pub fn check_rules(graph: &ArcGraph, config: &ArcConfig) -> CheckResult {
    let violations = config
        .rules
        .iter()
        .filter(|rule| !matches!(rule_severity(rule), Severity::Ignore))
        .flat_map(|rule| match rule {
            Rule::ForbiddenDependency { .. } => check_forbidden(graph, rule),
            Rule::NoCycles { .. } => check_cycles(graph, rule),
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
            let source_path = module_path(source, graph);
            let target_path = module_path(target, graph);
            let locations = match edge {
                Edge::ModuleDep { locations, .. } => locations.clone(),
                _ => Vec::new(),
            };
            Some(Violation {
                rule_name: name.clone(),
                rule_type: "forbidden-dependency".into(),
                severity: *severity,
                message: format!("{source_path} → {target_path}"),
                locations,
            })
        })
        .collect()
}

/// Check a `no-cycles` rule: find elementary cycles within the scoped subgraph.
fn check_cycles(graph: &ArcGraph, rule: &Rule) -> Vec<Violation> {
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
    let subgraph = graph.filter_map(
        |idx, _| scope_set.contains(&idx).then_some(idx),
        |_, edge| edge.is_production_module_dep().then_some(()),
    );

    subgraph
        .elementary_cycles()
        .into_iter()
        .map(|cycle| {
            let path_names: Vec<String> = cycle
                .path
                .iter()
                .map(|&idx| module_path(idx, graph))
                .collect();
            let message = format!("{} → {}", path_names.join(" → "), path_names[0]);
            Violation {
                rule_name: name.clone(),
                rule_type: "no-cycles".into(),
                severity: *severity,
                message,
                locations: Vec::new(),
            }
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

            let source_path = module_path(source, graph);
            let target_path = module_path(target, graph);
            let locations = match edge {
                Edge::ModuleDep { locations, .. } => locations.clone(),
                _ => Vec::new(),
            };
            Some(Violation {
                rule_name: name.clone(),
                rule_type: "layers".into(),
                severity: *severity,
                message: format!("{source_path} → {target_path}"),
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
        assert!(violations[0].message.contains("service"));
        assert!(violations[0].message.contains("db"));
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
        let violations = check_cycles(&graph, &rule);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("→"));
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
        let violations = check_cycles(&graph, &rule);
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
        let violations = check_cycles(&graph, &rule);
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
        let violations = check_cycles(&graph, &rule);
        assert_eq!(violations.len(), 2);
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
        assert!(violations[0].message.contains("db"));
        assert!(violations[0].message.contains("service"));
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
        let result = check_rules(&graph, &config);
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
        let result = check_rules(&graph, &config);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn test_check_result_has_errors() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                message: "a → b".into(),
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
                message: "cycle".into(),
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
        let result = check_rules(&graph, &config);
        assert!(result.violations.is_empty());
    }
}
