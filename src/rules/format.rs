//! Violation formatting as compiler-style diagnostics
//!
//! Formats `CheckResult` violations in a style similar to `rustc` error output:
//! `error[rule-type]: rule-name` with optional source locations and a summary line.

use crate::rules::config::Severity;
use crate::rules::engine::{CheckResult, CycleCluster, ViolationDetail};
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
        match &violation.detail {
            ViolationDetail::Edge { from, to } => {
                let _ = writeln!(output, "  = {from} → {to}");
            }
            ViolationDetail::Cluster(cluster) => {
                output.push_str(&cluster_block(cluster, "  "));
            }
        }
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

/// Format a cluster-level cycle report (default verbosity).
///
/// One block per SCC cluster ordered by feedback edge count: a header, then
/// either a single-cycle body (the ring plus its thinnest edge) or a tangle
/// body (the ranked feedback edges), followed by a summary line. Returns an
/// empty string when there are no clusters.
#[must_use]
pub fn format_cluster_report(clusters: &[CycleCluster]) -> String {
    use std::collections::HashSet;

    if clusters.is_empty() {
        return String::new();
    }
    let mut out = String::new();

    for (i, cluster) in clusters.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(out);
        }
        out.push_str(&cluster_block(cluster, ""));
    }

    let total_cycles: usize = clusters.iter().map(|c| c.cycles).sum();
    let crates: HashSet<&str> = clusters.iter().map(|c| c.crate_name.as_str()).collect();
    let _ = writeln!(out);
    // No total over the feedback sets: different clusters' sets have nothing to
    // do with each other, so their sum reads as a to-do list without measuring
    // anything.
    let _ = writeln!(
        out,
        "Summary: {}, {} across {}",
        plural(clusters.len(), "cluster"),
        plural(total_cycles, "cycle"),
        plural(crates.len(), "crate"),
    );
    out
}

/// Render one cluster: header, then either a single-cycle body (the ring plus
/// its thinnest edge) or a tangle body (the ranked feedback edges). `indent`
/// prefixes the header; the body indents further relative to it, as before.
fn cluster_block(cluster: &CycleCluster, indent: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{indent}cluster {}/{}: {} ({}, {})",
        cluster.position,
        cluster.total,
        cluster.place,
        plural(cluster.modules, "module"),
        plural(cluster.cycles, "cycle"),
    );

    if let Some(names) = &cluster.ring {
        let _ = writeln!(
            out,
            "{indent}  cycle: {} -> {}",
            names.join(" -> "),
            names[0]
        );
        if let Some(edge) = cluster.feedback_edges.first() {
            let _ = writeln!(
                out,
                "{indent}  fewest symbols: {} -> {} ({})",
                edge.from,
                edge.to,
                plural(edge.refs, "symbol"),
            );
        }
    } else {
        let from_width = cluster
            .feedback_edges
            .iter()
            .map(|edge| edge.from.len())
            .max()
            .unwrap_or(0);
        let to_width = cluster
            .feedback_edges
            .iter()
            .map(|edge| edge.to.len())
            .max()
            .unwrap_or(0);
        let cycles_width = cluster
            .feedback_edges
            .iter()
            .map(|edge| edge.cycles.to_string().len())
            .max()
            .unwrap_or(0);
        // Only claim an order when the cycle counts actually differ.
        let counts = || cluster.feedback_edges.iter().map(|edge| edge.cycles);
        let heading = if counts().min() == counts().max() {
            "edges:"
        } else {
            "edges, most cycles first:"
        };
        let _ = writeln!(out, "{indent}  {heading}");
        for edge in &cluster.feedback_edges {
            let cycle_word = if edge.cycles == 1 { "cycle" } else { "cycles" };
            let cycles = edge.cycles;
            let refs = plural(edge.refs, "symbol");
            let (from, to) = (&edge.from, &edge.to);
            let _ = writeln!(
                out,
                "{indent}    {from:<from_width$} -> {to:<to_width$} (on {cycles:>cycles_width$} {cycle_word}, {refs})"
            );
        }
        // Closes with a property of the cycles (the listed edges are a
        // hitting set), not with an instruction to remove them. Dropped
        // when edges and cycles pair off one to one: there the sentence
        // only restates the counts already in the list.
        let count = cluster.feedback_edges.len();
        let paired_off = count == cluster.cycles && counts().all(|c| c == 1);
        if !paired_off {
            let _ = if count == 1 {
                writeln!(out, "{indent}  every cycle contains this edge")
            } else {
                writeln!(
                    out,
                    "{indent}  every cycle contains at least one of these {count} edges"
                )
            };
        }
    }
    out
}

/// `"{n} {base}"`, pluralizing `base` with a trailing `s` unless `n == 1`.
fn plural(n: usize, base: &str) -> String {
    format!("{n} {base}{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceLocation;
    use crate::rules::engine::{CycleClusterEdge, Violation};
    use std::path::PathBuf;

    /// Single-cycle, single-edge `CycleCluster` fixture for tests that only
    /// care about a `no-cycles` violation being present, not its cluster detail.
    fn cluster_fixture(from: &str, to: &str) -> CycleCluster {
        CycleCluster {
            position: 1,
            total: 1,
            crate_name: "app".into(),
            place: "app".into(),
            modules: 2,
            cycles: 1,
            ring: Some(vec![from.into(), to.into()]),
            feedback_edges: vec![CycleClusterEdge {
                from: from.into(),
                to: to.into(),
                cycles: 1,
                refs: 1,
            }],
        }
    }

    #[test]
    fn test_format_single_error() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "no infra in domain".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                detail: ViolationDetail::Edge {
                    from: "domain::service".into(),
                    to: "infra::db".into(),
                },
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
                detail: ViolationDetail::Cluster(cluster_fixture("a", "b")),
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
                detail: ViolationDetail::Edge {
                    from: "a".into(),
                    to: "b".into(),
                },
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/domain/service.rs"),
                    line: 42,
                    symbols: vec![],
                    module_path: String::new(),
                    via_reexport: false,
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
                    detail: ViolationDetail::Edge {
                        from: "a".into(),
                        to: "b".into(),
                    },
                    locations: vec![],
                },
                Violation {
                    rule_name: "rule2".into(),
                    rule_type: "no-cycles".into(),
                    severity: Severity::Warn,
                    detail: ViolationDetail::Cluster(cluster_fixture("c", "d")),
                    locations: vec![],
                },
                Violation {
                    rule_name: "rule3".into(),
                    rule_type: "layers".into(),
                    severity: Severity::Error,
                    detail: ViolationDetail::Edge {
                        from: "x".into(),
                        to: "y".into(),
                    },
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

    #[test]
    fn test_format_cluster_detail_renders_under_no_cycles_header() {
        let cluster = CycleCluster {
            position: 1,
            total: 1,
            crate_name: "app".into(),
            place: "app".into(),
            modules: 2,
            cycles: 1,
            ring: Some(vec!["a".into(), "b".into()]),
            feedback_edges: vec![CycleClusterEdge {
                from: "a".into(),
                to: "b".into(),
                cycles: 1,
                refs: 1,
            }],
        };
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "no cycles in domain".into(),
                rule_type: "no-cycles".into(),
                severity: Severity::Error,
                detail: ViolationDetail::Cluster(cluster),
                locations: vec![],
            }],
        };
        let output = format_violations(&result);
        assert!(output.contains("error[no-cycles]: no cycles in domain"));
        assert!(output.contains("  cluster 1/1: app (2 modules, 1 cycle)"));
        assert!(output.contains("    cycle: a -> b -> a"));
    }

    use crate::graph::{ArcGraph, Edge, Node};

    // ===== format_cluster_report tests =====

    use crate::diagnose::MinimalCycles;
    use crate::model::EdgeContext;

    /// Single-crate graph "app" with modules by name and production `ModuleDep`
    /// edges `(from, to, ref_count)`.
    fn cyc_graph(modules: &[&str], deps: &[(usize, usize, usize)]) -> ArcGraph {
        let mut g = ArcGraph::new();
        let crate_idx = g.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let idx: Vec<_> = modules
            .iter()
            .map(|m| {
                let n = g.add_node(Node::Module {
                    name: (*m).into(),
                    crate_idx,
                });
                g.add_edge(crate_idx, n, Edge::Contains);
                n
            })
            .collect();
        for &(from, to, refs) in deps {
            // One symbol per line, so the edge reads the same whether refs
            // counts sites or symbols.
            let locations = (0..refs)
                .map(|i| SourceLocation {
                    file: format!("src/{}.rs", modules[from]).into(),
                    line: i + 1,
                    symbols: vec![format!("Sym{i}")],
                    module_path: String::new(),
                    via_reexport: false,
                })
                .collect();
            g.add_edge(
                idx[from],
                idx[to],
                Edge::ModuleDep {
                    locations,
                    context: EdgeContext::production(),
                },
            );
        }
        g
    }

    fn report_of(g: &ArcGraph) -> Vec<CycleCluster> {
        let sub = g.production_subgraph();
        let analysis = sub.minimal_cycles();
        let report = g.cluster_report(&sub, &analysis);
        let total = report.clusters.len();
        report
            .clusters
            .iter()
            .enumerate()
            .map(|(i, cluster)| CycleCluster::from_cluster(g, &analysis, cluster, i + 1, total))
            .collect()
    }

    #[test]
    fn cluster_report_single_cycle_block() {
        let g = cyc_graph(&["a", "b"], &[(0, 1, 1), (1, 0, 3)]);
        let clusters = report_of(&g);
        let out = format_cluster_report(&clusters);
        assert!(
            out.contains("cluster 1/1: app (2 modules, 1 cycle)"),
            "got:\n{out}"
        );
        assert!(out.contains("cycle: a -> b -> a"), "got:\n{out}");
        assert!(
            out.contains("fewest symbols: a -> b (1 symbol)"),
            "got:\n{out}"
        );
        assert!(
            out.contains("Summary: 1 cluster, 1 cycle across 1 crate"),
            "got:\n{out}"
        );
    }

    #[test]
    fn cluster_report_tangle_block() {
        // Two triangles share a->b, plus a separate a<->e ring on the same
        // node: three cycles, two feedback edges carrying two and one cycle.
        let g = cyc_graph(
            &["a", "b", "c", "d", "e"],
            &[
                (0, 1, 1),
                (1, 2, 1),
                (2, 0, 1),
                (1, 3, 1),
                (3, 0, 1),
                (0, 4, 1),
                (4, 0, 2),
            ],
        );
        let clusters = report_of(&g);
        let out = format_cluster_report(&clusters);
        assert!(out.contains("(5 modules, 3 cycles)"), "got:\n{out}");
        assert!(out.contains("edges, most cycles first:"), "got:\n{out}");
        assert!(out.contains("(on 2 cycles, 1 symbol)"), "got:\n{out}");
        assert!(
            out.contains("every cycle contains at least one of these 2 edges"),
            "got:\n{out}"
        );
        assert!(!out.contains("fewest symbols:"), "got:\n{out}");
    }

    #[test]
    fn cluster_report_drops_order_and_closing_line_when_cycles_pair_off() {
        // a<->b and a<->c share only node a: two cycles, one feedback edge
        // each. Nothing to order, and the closing line would just repeat the
        // counts already on the rows.
        let g = cyc_graph(
            &["a", "b", "c"],
            &[(0, 1, 1), (1, 0, 1), (0, 2, 1), (2, 0, 1)],
        );
        let clusters = report_of(&g);
        let out = format_cluster_report(&clusters);
        assert!(out.contains("  edges:"), "got:\n{out}");
        assert!(!out.contains("most cycles first"), "got:\n{out}");
        assert!(!out.contains("every cycle contains"), "got:\n{out}");
    }

    #[test]
    fn cluster_report_closing_line_is_singular_for_a_lone_edge() {
        // Two triangles sharing edge a->b: two cycles, but one edge hits both.
        let g = cyc_graph(
            &["a", "b", "c", "d"],
            &[(0, 1, 1), (1, 2, 1), (2, 0, 1), (1, 3, 1), (3, 0, 1)],
        );
        let clusters = report_of(&g);
        let out = format_cluster_report(&clusters);
        assert!(
            out.contains("every cycle contains this edge"),
            "got:\n{out}"
        );
    }

    #[test]
    fn cluster_report_summary_counts_clusters_cycles_and_crates() {
        let g = cyc_graph(
            &["a", "b", "c", "d"],
            &[(0, 1, 1), (1, 0, 1), (2, 3, 1), (3, 2, 1)],
        );
        let clusters = report_of(&g);
        let out = format_cluster_report(&clusters);
        assert!(
            out.contains("Summary: 2 clusters, 2 cycles across 1 crate"),
            "got:\n{out}"
        );
    }

    #[test]
    fn cluster_report_empty_is_blank() {
        let g = cyc_graph(&["a", "b"], &[(0, 1, 1)]);
        let clusters = report_of(&g);
        assert!(format_cluster_report(&clusters).is_empty());
    }
}
