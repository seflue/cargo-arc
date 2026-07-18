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

/// Format a cluster-level cycle report (default verbosity).
///
/// One block per SCC cluster ordered by cut-set size: a header, then either a
/// single-cycle body (the ring plus its cheapest cut) or a tangle body (the
/// ranked cut-set), followed by a summary line. Returns an empty string when
/// there are no clusters. Module names are shown relative to the cluster's
/// crate, which the header names.
#[must_use]
pub fn format_cluster_report(
    graph: &crate::graph::ArcGraph,
    analysis: &crate::diagnose::CycleAnalysis,
    report: &crate::diagnose::ClusterReport,
) -> String {
    use std::collections::HashSet;

    if report.clusters.is_empty() {
        return String::new();
    }
    let total = report.clusters.len();
    let mut out = String::new();

    for (i, cluster) in report.clusters.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(out);
        }
        let crate_name = graph[cluster.crate_idx].name().to_string();
        let _ = writeln!(
            out,
            "cluster {}/{}: {} ({}, {}, {} to break)",
            i + 1,
            total,
            crate_name,
            plural(cluster.nodes.len(), "module"),
            plural(cluster.cycles.len(), "cycle"),
            plural(cluster.cuts.len(), "cut"),
        );

        if cluster.cycles.len() == 1 {
            let cycle = &analysis.cycles[cluster.cycles[0]];
            let names: Vec<String> = cycle
                .path
                .iter()
                .map(|&n| rel_name(graph, n, &crate_name))
                .collect();
            let _ = writeln!(out, "  cycle: {} -> {}", names.join(" -> "), names[0]);
            if let Some(cut) = cluster.cuts.first() {
                let _ = writeln!(
                    out,
                    "  fewest symbols: {} -> {} ({})",
                    rel_name(graph, cut.from, &crate_name),
                    rel_name(graph, cut.to, &crate_name),
                    plural(cut.refs, "symbol"),
                );
            }
        } else {
            let names: Vec<(String, String)> = cluster
                .cuts
                .iter()
                .map(|cut| {
                    (
                        rel_name(graph, cut.from, &crate_name),
                        rel_name(graph, cut.to, &crate_name),
                    )
                })
                .collect();
            let from_width = names.iter().map(|(from, _)| from.len()).max().unwrap_or(0);
            let to_width = names.iter().map(|(_, to)| to.len()).max().unwrap_or(0);
            let breaks_width = cluster
                .cuts
                .iter()
                .map(|cut| cut.breaks.to_string().len())
                .max()
                .unwrap_or(0);
            for (cut, (from, to)) in cluster.cuts.iter().zip(&names) {
                let cycle_word = if cut.breaks == 1 { "cycle" } else { "cycles" };
                let breaks = cut.breaks;
                let refs = plural(cut.refs, "symbol");
                let _ = writeln!(
                    out,
                    "    {from:<from_width$} -> {to:<to_width$} (on {breaks:>breaks_width$} {cycle_word}, {refs})"
                );
            }
        }
    }

    let total_cycles: usize = report.clusters.iter().map(|c| c.cycles.len()).sum();
    let total_cuts: usize = report.clusters.iter().map(|c| c.cuts.len()).sum();
    let crates: HashSet<_> = report.clusters.iter().map(|c| c.crate_idx).collect();
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Summary: {}, {} across {} - {} break all cycles",
        plural(total, "cluster"),
        plural(total_cycles, "cycle"),
        plural(crates.len(), "crate"),
        plural(total_cuts, "cut"),
    );
    out
}

/// `"{n} {base}"`, pluralizing `base` with a trailing `s` unless `n == 1`.
fn plural(n: usize, base: &str) -> String {
    format!("{n} {base}{}", if n == 1 { "" } else { "s" })
}

/// Fully-qualified module name with the leading `<crate>::` stripped, since the
/// cluster header already names the crate.
fn rel_name(
    graph: &crate::graph::ArcGraph,
    idx: petgraph::graph::NodeIndex,
    crate_name: &str,
) -> String {
    let qualified = graph.qualified_name(idx);
    qualified
        .strip_prefix(&format!("{crate_name}::"))
        .map(String::from)
        .unwrap_or(qualified)
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

    use crate::graph::{ArcGraph, Edge, Node};

    // ===== format_cluster_report tests =====

    use crate::diagnose::{ClusterReport, CycleAnalysis, MinimalCycles};
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

    fn report_of(g: &ArcGraph) -> (CycleAnalysis, ClusterReport) {
        let analysis = g.production_subgraph().minimal_cycles();
        let report = g.cluster_report(&analysis, true);
        (analysis, report)
    }

    #[test]
    fn cluster_report_single_cycle_block() {
        let g = cyc_graph(&["a", "b"], &[(0, 1, 1), (1, 0, 3)]);
        let (analysis, report) = report_of(&g);
        let out = format_cluster_report(&g, &analysis, &report);
        assert!(
            out.contains("cluster 1/1: app (2 modules, 1 cycle, 1 cut to break)"),
            "got:\n{out}"
        );
        assert!(out.contains("cycle: a -> b -> a"), "got:\n{out}");
        assert!(
            out.contains("fewest symbols: a -> b (1 symbol)"),
            "got:\n{out}"
        );
        assert!(
            out.contains("Summary: 1 cluster, 1 cycle across 1 crate - 1 cut break all cycles"),
            "got:\n{out}"
        );
    }

    #[test]
    fn cluster_report_tangle_block() {
        // a<->b and a<->c share node a: one cluster, two cycles, two cuts.
        let g = cyc_graph(
            &["a", "b", "c"],
            &[(0, 1, 1), (1, 0, 1), (0, 2, 1), (2, 0, 1)],
        );
        let (analysis, report) = report_of(&g);
        let out = format_cluster_report(&g, &analysis, &report);
        assert!(
            out.contains("(3 modules, 2 cycles, 2 cuts to break)"),
            "got:\n{out}"
        );
        assert!(out.contains("(on 1 cycle, 1 symbol)"), "got:\n{out}");
        assert!(!out.contains("fewest symbols:"), "got:\n{out}");
    }

    #[test]
    fn cluster_report_summary_counts_crates_and_cuts() {
        let g = cyc_graph(
            &["a", "b", "c", "d"],
            &[(0, 1, 1), (1, 0, 1), (2, 3, 1), (3, 2, 1)],
        );
        let (analysis, report) = report_of(&g);
        let out = format_cluster_report(&g, &analysis, &report);
        assert!(
            out.contains("Summary: 2 clusters, 2 cycles across 1 crate - 2 cuts break all cycles"),
            "got:\n{out}"
        );
    }

    #[test]
    fn cluster_report_empty_is_blank() {
        let g = cyc_graph(&["a", "b"], &[(0, 1, 1)]);
        let (analysis, report) = report_of(&g);
        assert!(format_cluster_report(&g, &analysis, &report).is_empty());
    }
}
