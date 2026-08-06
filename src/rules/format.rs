//! Violation formatting as compiler-style diagnostics
//!
//! Formats `CheckResult` violations in a style similar to `rustc` error output:
//! `error[rule-type]: rule-name` with optional source locations and a summary line.

use crate::rules::baseline::FindingKey;
use crate::rules::config::{DiagnosticLevel, Severity};
use crate::rules::diagnostics::{Diagnostic, DiagnosticKind};
use crate::rules::engine::{CheckResult, CycleCluster, Violation, ViolationDetail};
use std::fmt::Write;

/// Format all violations as compiler-style diagnostics.
///
/// Returns an empty string when there are no violations and nothing was
/// suppressed or baselined. Otherwise produces one diagnostic block per
/// violation, followed by either the suppressed/baselined findings (when
/// `show_suppressed`) or a one-line count of them, then a summary line over
/// the reported findings.
#[must_use]
pub fn format_violations(result: &CheckResult, show_suppressed: bool) -> String {
    if result.violations.is_empty()
        && result.suppressed.is_empty()
        && result.baselined.is_empty()
        && result.diagnostics.is_empty()
    {
        return String::new();
    }

    let mut output = String::new();
    for violation in &result.violations {
        let level = match violation.severity {
            Severity::Error => "error",
            Severity::Warn => "warning",
            Severity::Ignore => continue,
        };
        violation_block(&mut output, violation, level);
    }

    if show_suppressed {
        for violation in &result.suppressed {
            violation_block(&mut output, violation, "except");
        }
        for violation in &result.baselined {
            violation_block(&mut output, violation, "baseline");
        }
    } else if !result.suppressed.is_empty() || !result.baselined.is_empty() {
        let except_count = result.suppressed.len();
        let baseline_count = result.baselined.len();
        match (except_count, baseline_count) {
            (n, 0) => {
                let _ = writeln!(
                    output,
                    "{} allowed by except, not counted",
                    plural(n, "finding")
                );
            }
            (0, n) => {
                let _ = writeln!(
                    output,
                    "{} frozen in the baseline, not counted",
                    plural(n, "finding")
                );
            }
            (a, b) => {
                let _ = writeln!(
                    output,
                    "{} hidden ({a} allowed by except, {b} frozen in the baseline)",
                    plural(a + b, "finding"),
                );
            }
        }
        let _ = writeln!(output, "  arc check --show-suppressed lists them");
    }

    output.push_str(&diagnostics_block(&result.diagnostics));

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
    // Only when something is actually reported: a run whose findings are all
    // covered by `except` exits 0, and an `error:` line would read as a
    // failure in CI logs.
    if errors + warnings > 0 {
        let _ = writeln!(output, "error: {errors} error(s), {warnings} warning(s)");
    }
    output
}

/// One block for the gaps in the configuration, headed `warning:` or, as soon
/// as a `deny` is among them, `error:`. Entries that share a diagnostic and an
/// explanation take one line together: nineteen unlayered crates are one gap,
/// not nineteen.
fn diagnostics_block(diagnostics: &[Diagnostic]) -> String {
    /// Beyond this many subjects on one line the list stops informing.
    const SHOWN: usize = 5;

    if diagnostics.is_empty() {
        return String::new();
    }
    let denied = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.level == DiagnosticLevel::Deny);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}: configuration",
        if denied { "error" } else { "warning" }
    );

    let same_line =
        |a: &Diagnostic, b: &Diagnostic| a.name() == b.name() && explanation(a) == explanation(b);
    for group in diagnostics.chunk_by(same_line) {
        let subjects: Vec<String> = group.iter().map(subject).collect();
        let shown = subjects.len().min(SHOWN);
        let _ = write!(
            out,
            "  {}: {}",
            group[0].name(),
            subjects[..shown].join(", ")
        );
        if let Some(hidden) = subjects.len().checked_sub(SHOWN).filter(|&n| n > 0) {
            let _ = write!(out, ", ... {hidden} more");
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "    {}", explanation(&group[0]));
    }
    let _ = writeln!(out);
    out
}

/// What the diagnostic is about: the crate, the frozen finding, the pattern.
fn subject(diagnostic: &Diagnostic) -> String {
    match &diagnostic.kind {
        DiagnosticKind::UnlayeredCrate { krate } => krate.clone(),
        DiagnosticKind::UnmatchedBaselineEntry { entry } => {
            format!("{}: {}", entry.rule, finding(&entry.key))
        }
        DiagnosticKind::UnmatchedExcept { entry } => entry.pattern.clone(),
    }
}

/// Why the state is worth a word, and what closes it.
fn explanation(diagnostic: &Diagnostic) -> String {
    match &diagnostic.kind {
        DiagnosticKind::UnlayeredCrate { .. } => {
            "in no layer, so its edges go unchecked".to_string()
        }
        DiagnosticKind::UnmatchedBaselineEntry { .. } => {
            "suppresses nothing; arc check --generate-baseline rewrites the baseline".to_string()
        }
        DiagnosticKind::UnmatchedExcept { entry } => {
            format!("in rule {:?}, matches no module", entry.rule)
        }
    }
}

fn finding(key: &FindingKey) -> String {
    match key {
        FindingKey::Edge { from, to } => format!("{from} → {to}"),
        FindingKey::Ring(members) => format!("{} -> {}", members.join(" -> "), members[0]),
    }
}

/// Render one diagnostic block: `{level}[rule-type]: rule-name`, its source
/// locations, and its edge/cluster/ring detail. `level` is `error`/`warning`
/// for reported violations, `except` for suppressed ones, `baseline` for
/// baselined ones: the word names the mechanism that let them through.
fn violation_block(out: &mut String, violation: &Violation, level: &str) {
    let _ = writeln!(
        out,
        "{level}[{}]: {}",
        violation.rule_type, violation.rule_name
    );
    for loc in &violation.locations {
        let _ = writeln!(out, "  --> {}:{}", loc.file.display(), loc.line);
    }
    match &violation.detail {
        ViolationDetail::Edge { from, to } => {
            let _ = writeln!(out, "  = {from} → {to}");
        }
        ViolationDetail::Cluster(cluster) => {
            out.push_str(&cluster_block(cluster, "  "));
        }
        ViolationDetail::Ring { modules } => {
            let _ = writeln!(out, "  = {} -> {}", modules.join(" -> "), modules[0]);
        }
    }
    let _ = writeln!(out);
}

/// Format a cluster-level cycle report (default verbosity).
///
/// One block per SCC cluster ordered by feedback edge count: a header, then
/// either a single-cycle body (the ring plus the edge carrying the fewest
/// symbols) or a tangle body (the ranked feedback edges), followed by a summary
/// line. Returns an empty string when there are no clusters.
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
/// the edge carrying the fewest symbols) or a tangle body (the ranked feedback
/// edges). `indent` prefixes the header; the body indents further relative to
/// it, as before.
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
                plural(edge.symbols, "symbol"),
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
            let symbols = plural(edge.symbols, "symbol");
            let (from, to) = (&edge.from, &edge.to);
            let _ = writeln!(
                out,
                "{indent}    {from:<from_width$} -> {to:<to_width$} (on {cycles:>cycles_width$} {cycle_word}, {symbols})"
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
pub(crate) fn plural(n: usize, base: &str) -> String {
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
                symbols: 1,
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
            ..Default::default()
        };
        let output = format_violations(&result, false);
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
            ..Default::default()
        };
        let output = format_violations(&result, false);
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
            ..Default::default()
        };
        let output = format_violations(&result, false);
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
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(output.contains("error: 2 error(s), 1 warning(s)"));
    }

    #[test]
    fn test_format_empty() {
        let result = CheckResult::default();
        let output = format_violations(&result, false);
        assert!(output.is_empty());
    }

    fn suppressed_edge_violation() -> Violation {
        Violation {
            rule_name: "no infra in domain".into(),
            rule_type: "forbidden-dependency".into(),
            severity: Severity::Error,
            detail: ViolationDetail::Edge {
                from: "domain::service".into(),
                to: "infra::db".into(),
            },
            locations: vec![],
        }
    }

    #[test]
    fn test_format_suppressed_hidden_by_default_but_counted() {
        let result = CheckResult {
            suppressed: vec![suppressed_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            !output.contains("except[forbidden-dependency]"),
            "got:\n{output}"
        );
        assert!(
            output.contains("1 finding allowed by except, not counted"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_suppressed_shown_with_flag() {
        let result = CheckResult {
            suppressed: vec![suppressed_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert!(
            output.contains("except[forbidden-dependency]: no infra in domain"),
            "got:\n{output}"
        );
        assert!(
            output.contains("= domain::service → infra::db"),
            "got:\n{output}"
        );
        assert!(!output.contains("not counted"), "got:\n{output}");
    }

    #[test]
    fn test_format_no_suppressed_output_unchanged() {
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
            ..Default::default()
        };
        let without_flag = format_violations(&result, false);
        let with_flag = format_violations(&result, true);
        assert_eq!(without_flag, with_flag);
        assert!(!without_flag.contains("except"), "got:\n{without_flag}");
        assert!(
            !without_flag.contains("not counted"),
            "got:\n{without_flag}"
        );
    }

    fn baselined_ring_violation() -> Violation {
        Violation {
            rule_name: "no cycles in domain".into(),
            rule_type: "no-cycles".into(),
            severity: Severity::Error,
            detail: ViolationDetail::Ring {
                modules: vec!["a".into(), "b".into()],
            },
            locations: vec![],
        }
    }

    #[test]
    fn test_format_baselined_hidden_by_default_but_counted() {
        let result = CheckResult {
            baselined: vec![baselined_ring_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(!output.contains("baseline[no-cycles]"), "got:\n{output}");
        assert!(
            output.contains("1 finding frozen in the baseline, not counted"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_baselined_shown_with_flag() {
        let result = CheckResult {
            suppressed: vec![suppressed_edge_violation()],
            baselined: vec![baselined_ring_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert!(
            output.contains("except[forbidden-dependency]: no infra in domain"),
            "got:\n{output}"
        );
        assert!(
            output.contains("baseline[no-cycles]: no cycles in domain"),
            "got:\n{output}"
        );
        assert!(output.contains("= a -> b -> a"), "got:\n{output}");
        assert!(!output.contains("not counted"), "got:\n{output}");
    }

    #[test]
    fn test_format_both_hidden_counts_combined() {
        let result = CheckResult {
            suppressed: vec![suppressed_edge_violation()],
            baselined: vec![baselined_ring_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("2 findings hidden (1 allowed by except, 1 frozen in the baseline)"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_only_baselined_no_reported_violations() {
        let result = CheckResult {
            baselined: vec![baselined_ring_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(!output.is_empty());
        assert!(
            !output.contains("error:"),
            "a green run must not print an error line, got:\n{output}"
        );
    }

    #[test]
    fn test_format_only_suppressed_no_reported_violations() {
        let result = CheckResult {
            suppressed: vec![suppressed_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(!output.is_empty());
        assert!(
            !output.contains("error:"),
            "a green run must not print an error line, got:\n{output}"
        );
    }

    // ===== diagnostics =====

    use crate::rules::baseline::{BaselineEntry, FindingKey};
    use crate::rules::config::DiagnosticLevel;
    use crate::rules::diagnostics::{DeadExcept, Diagnostic, DiagnosticKind};

    fn unlayered(krate: &str, level: DiagnosticLevel) -> Diagnostic {
        Diagnostic {
            level,
            kind: DiagnosticKind::UnlayeredCrate {
                krate: krate.into(),
            },
        }
    }

    #[test]
    fn test_format_unlayered_crates_share_one_line() {
        let result = CheckResult {
            diagnostics: vec![
                unlayered("benches", DiagnosticLevel::Warn),
                unlayered("xtask", DiagnosticLevel::Warn),
            ],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(output.contains("warning: configuration"), "got:\n{output}");
        assert!(
            output.contains("  unlayered-crate: benches, xtask"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_denied_diagnostic_heads_the_block_with_error() {
        let result = CheckResult {
            diagnostics: vec![unlayered("xtask", DiagnosticLevel::Deny)],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(output.contains("error: configuration"), "got:\n{output}");
    }

    #[test]
    fn test_format_unmatched_except_names_its_rule() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warn,
                kind: DiagnosticKind::UnmatchedExcept {
                    entry: DeadExcept {
                        rule: "no infra in domain".into(),
                        pattern: "domain::lgacy".into(),
                    },
                },
            }],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("  unmatched-except: domain::lgacy"),
            "got:\n{output}"
        );
        assert!(
            output.contains(r#"in rule "no infra in domain""#),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_unmatched_baseline_entry_shows_the_finding() {
        let result = CheckResult {
            diagnostics: vec![
                Diagnostic {
                    level: DiagnosticLevel::Warn,
                    kind: DiagnosticKind::UnmatchedBaselineEntry {
                        entry: BaselineEntry {
                            rule: "no infra in domain".into(),
                            key: FindingKey::edge("domain::service", "infra::db"),
                        },
                    },
                },
                Diagnostic {
                    level: DiagnosticLevel::Warn,
                    kind: DiagnosticKind::UnmatchedBaselineEntry {
                        entry: BaselineEntry {
                            rule: "domain acyclic".into(),
                            key: FindingKey::ring(vec![
                                "domain::a".to_string(),
                                "domain::b".to_string(),
                            ]),
                        },
                    },
                },
            ],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("domain::service → infra::db"),
            "got:\n{output}"
        );
        assert!(
            output.contains("domain::a -> domain::b -> domain::a"),
            "got:\n{output}"
        );
        assert!(output.contains("--generate-baseline"), "got:\n{output}");
    }

    #[test]
    fn test_format_long_subject_list_is_cut() {
        let names = ["a", "b", "c", "d", "e", "f", "g"];
        let result = CheckResult {
            diagnostics: names
                .iter()
                .map(|name| unlayered(name, DiagnosticLevel::Warn))
                .collect(),
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("a, b, c, d, e, ... 2 more"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_diagnostics_do_not_count_as_rule_violations() {
        let result = CheckResult {
            diagnostics: vec![unlayered("xtask", DiagnosticLevel::Deny)],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            !output.contains("error(s)"),
            "the summary line counts rule violations, got:\n{output}"
        );
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
                symbols: 1,
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
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(output.contains("error[no-cycles]: no cycles in domain"));
        assert!(output.contains("  cluster 1/1: app (2 modules, 1 cycle)"));
        assert!(output.contains("    cycle: a -> b -> a"));
    }

    use crate::graph::{ArcGraph, Edge, Node, Reexports};

    // ===== format_cluster_report tests =====

    use crate::diagnose::RepresentativeCycles;
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
        for &(from, to, symbols) in deps {
            // One symbol per line, so the edge reads the same whether the count
            // takes sites or symbols.
            let locations = (0..symbols)
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
        let sub = g.production_subgraph(Reexports::Included);
        let analysis = sub.representative_cycles();
        let report = g.cluster_report(&sub, &analysis, |_| false);
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
