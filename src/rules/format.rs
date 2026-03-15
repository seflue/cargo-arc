//! Violation formatting as compiler-style diagnostics
//!
//! Formats `CheckResult` violations in a style similar to `rustc` error output:
//! `error[rule-type]: rule-name` with optional source locations and a summary line.

use crate::rules::config::Severity;
use crate::rules::engine::CheckResult;
use std::fmt::Write;

/// Format all violations as compiler-style diagnostics.
///
/// Returns an empty string when there are no violations. Otherwise produces
/// one diagnostic block per violation followed by a summary line.
#[must_use]
pub fn format_violations(result: &CheckResult) -> String {
    if result.violations.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for violation in &result.violations {
        let level = match violation.severity {
            Severity::Error => "error",
            Severity::Warn => "warning",
            Severity::Ignore => continue,
        };
        let _ = writeln!(
            output,
            "{level}[{}]: {}",
            violation.rule_type, violation.rule_name
        );
        for loc in &violation.locations {
            let _ = writeln!(output, "  --> {}:{}", loc.file.display(), loc.line);
        }
        let _ = writeln!(output, "  = {}", violation.message);
        let _ = writeln!(output);
    }

    let errors = result
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Error)
        .count();
    let warnings = result
        .violations
        .iter()
        .filter(|v| v.severity == Severity::Warn)
        .count();
    let _ = writeln!(output, "error: {errors} error(s), {warnings} warning(s)");
    output
}

/// Format detected cycles as compiler-style error messages (legacy format).
///
/// Returns an empty string when `cycles` is empty. Otherwise produces one
/// `error[cycle]:` line per cycle (using `<->` for direct / `->` chains for
/// transitive) followed by a summary line.
#[must_use]
pub fn format_cycle_errors(
    graph: &crate::graph::ArcGraph,
    cycles: &[crate::layout::Cycle],
) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceLocation;
    use crate::rules::engine::Violation;
    use std::path::PathBuf;

    #[test]
    fn test_format_single_error() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "no infra in domain".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                message: "domain::service → infra::db".into(),
                locations: vec![],
            }],
        };
        let output = format_violations(&result);
        assert!(output.contains("error[forbidden-dependency]: no infra in domain"));
        assert!(output.contains("= domain::service → infra::db"));
    }

    #[test]
    fn test_format_warning() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "no cycles in domain".into(),
                rule_type: "no-cycles".into(),
                severity: Severity::Warn,
                message: "a → b → a".into(),
                locations: vec![],
            }],
        };
        let output = format_violations(&result);
        assert!(output.contains("warning[no-cycles]: no cycles in domain"));
    }

    #[test]
    fn test_format_with_location() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "test".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                message: "a → b".into(),
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/domain/service.rs"),
                    line: 42,
                    symbols: vec![],
                    module_path: String::new(),
                }],
            }],
        };
        let output = format_violations(&result);
        assert!(output.contains("--> src/domain/service.rs:42"));
    }

    #[test]
    fn test_format_summary() {
        let result = CheckResult {
            violations: vec![
                Violation {
                    rule_name: "rule1".into(),
                    rule_type: "forbidden-dependency".into(),
                    severity: Severity::Error,
                    message: "a → b".into(),
                    locations: vec![],
                },
                Violation {
                    rule_name: "rule2".into(),
                    rule_type: "no-cycles".into(),
                    severity: Severity::Warn,
                    message: "cycle".into(),
                    locations: vec![],
                },
                Violation {
                    rule_name: "rule3".into(),
                    rule_type: "layers".into(),
                    severity: Severity::Error,
                    message: "x → y".into(),
                    locations: vec![],
                },
            ],
        };
        let output = format_violations(&result);
        assert!(output.contains("error: 2 error(s), 1 warning(s)"));
    }

    #[test]
    fn test_format_empty() {
        let result = CheckResult { violations: vec![] };
        let output = format_violations(&result);
        assert!(output.is_empty());
    }

    // ===== format_cycle_errors tests (migrated from cli.rs) =====

    use crate::graph::{ArcGraph, Node};
    use crate::layout::Cycle;
    use petgraph::graph::NodeIndex;

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
}
