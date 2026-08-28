//! Violation formatting as compiler-style output
//!
//! Formats `CheckResult` violations in a style similar to `rustc` error output:
//! `error[rule-type]: rule-name` with optional source locations and a summary line.

use crate::model::{Edge, EdgeSymbols};
use crate::rules::config::{DiagnosticLevel, Severity};
use crate::rules::diagnostics::{Diagnostic, DiagnosticKind};
use crate::rules::engine::{CheckResult, CycleCluster, Violation, ViolationDetail, ViolationState};
use std::fmt::Write;

/// Format all violations as compiler-style output.
///
/// Returns an empty string when there are no violations and nothing was
/// allowed or frozen. Otherwise produces one block per rule, holding its
/// reported violations, plus its allowed and frozen ones when `show_silenced`;
/// without the flag, a one-line count of everything silenced follows instead.
/// The per-rule counts belong to [`format_status`], which writes them to
/// stdout.
#[must_use]
pub fn format_violations(result: &CheckResult, show_silenced: bool) -> String {
    if result.violations.is_empty() && result.diagnostics.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for rule in result.violations.chunk_by(same_rule) {
        let level = match rule[0].severity {
            Severity::Error => "error",
            Severity::Warn => "warning",
            Severity::Ignore => continue,
        };
        let shown: Vec<&Violation> = if show_silenced {
            rule.iter().collect()
        } else {
            rule.iter()
                .filter(|violation| violation.state == ViolationState::Reported)
                .collect()
        };
        if shown.is_empty() {
            continue;
        }
        let has_reported = rule.iter().any(|v| v.state == ViolationState::Reported);
        let level = if has_reported { level } else { "silenced" };
        rule_block(&mut output, &shown, level);
    }

    let except_count = result.allowed().count();
    let baseline_count = result.frozen().count();
    if !show_silenced && except_count + baseline_count > 0 {
        match (except_count, baseline_count) {
            (n, 0) => {
                let _ = writeln!(
                    output,
                    "{} allowed by except, not counted",
                    plural(n, "violation")
                );
            }
            (0, n) => {
                let _ = writeln!(
                    output,
                    "{} frozen in the baseline, not counted",
                    plural(n, "violation")
                );
            }
            (a, b) => {
                let _ = writeln!(
                    output,
                    "{} silenced ({a} allowed by except, {b} frozen in the baseline)",
                    plural(a + b, "violation"),
                );
            }
        }
        let _ = writeln!(output, "  arc check --show-silenced lists them");
    }

    output.push_str(&diagnostics_block(&result.diagnostics));
    output
}

/// Render one status line per rule checked, plus one for the configuration.
///
/// `allowed` and `frozen` stay apart because one is meant to stay and the other
/// to shrink, so their sum would say nothing over time.
#[must_use]
pub fn format_status(result: &CheckResult) -> String {
    let mut output = String::new();
    for rule in &result.checked_rules {
        let reported = |severity| {
            of_rule(result.reported(), rule)
                .filter(|violation| violation.severity == severity)
                .count()
        };
        let errors = reported(Severity::Error);
        let warnings = reported(Severity::Warn);
        let _ = writeln!(
            output,
            "{rule} {}: {errors} errors, {warnings} warnings, {} allowed, {} frozen",
            status_word(errors, warnings),
            of_rule(result.allowed(), rule).count(),
            of_rule(result.frozen(), rule).count()
        );
    }

    let level = |wanted| {
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.level == wanted)
            .count()
    };
    // Without this line, a run whose rules are all `ok` would exit 1 over a
    // denied diagnostic with nothing on stdout saying why.
    let denied = level(DiagnosticLevel::Deny);
    let warned = level(DiagnosticLevel::Warn);
    let _ = writeln!(
        output,
        "config {}: {denied} errors, {warned} warnings",
        status_word(denied, warned)
    );
    output
}

fn of_rule<'a>(
    violations: impl Iterator<Item = &'a Violation>,
    rule: &'a str,
) -> impl Iterator<Item = &'a Violation> {
    violations.filter(move |violation| violation.rule_name == rule)
}

fn status_word(errors: usize, warnings: usize) -> &'static str {
    match (errors, warnings) {
        (0, 0) => "ok",
        (0, _) => "WARN",
        _ => "FAILED",
    }
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

/// What the diagnostic is about: the crate, the frozen violation, the pattern.
fn subject(diagnostic: &Diagnostic) -> String {
    match &diagnostic.kind {
        DiagnosticKind::UnlayeredCrate { krate } => krate.clone(),
        DiagnosticKind::UnmatchedBaselineEntry { entry } => {
            format!("{}: {}", entry.rule, edge(&entry.key.edge))
        }
        DiagnosticKind::WideBaselineEntry { entry, surplus } => format!(
            "{}: {} ({} no longer {})",
            entry.rule,
            edge(&entry.key.edge),
            symbol_list(surplus),
            if surplus.len() == 1 {
                "crosses"
            } else {
                "cross"
            }
        ),
        DiagnosticKind::UnmatchedExcept { entry } => entry.pattern.clone(),
        DiagnosticKind::UnmatchedPattern { entry } => entry.pattern.clone(),
    }
}

/// Why the state is worth a word, and what closes it.
fn explanation(diagnostic: &Diagnostic) -> String {
    match &diagnostic.kind {
        DiagnosticKind::UnlayeredCrate { .. } => {
            "in no layer, so its edges go unchecked".to_string()
        }
        DiagnosticKind::UnmatchedBaselineEntry { .. } => {
            "freezes nothing; arc check --generate-baseline rewrites the baseline".to_string()
        }
        DiagnosticKind::WideBaselineEntry { .. } => {
            "freezes more than the edge carries; arc check --generate-baseline narrows the entry"
                .to_string()
        }
        DiagnosticKind::UnmatchedExcept { entry } => {
            format!("in rule {:?}, matches no module", entry.rule)
        }
        DiagnosticKind::UnmatchedPattern { entry } => {
            format!(
                "in rule {:?}, matches no module: the rule checks nothing",
                entry.rule
            )
        }
    }
}

fn edge(edge: &Edge) -> String {
    format!("{} → {}", edge.from, edge.to)
}

/// Symbol names for a message, in the order the baseline file writes them. An
/// unnamed reference has nothing to print, so it is spelled out.
fn symbol_list(symbols: &EdgeSymbols) -> String {
    let mut names: Vec<&str> = symbols.named.iter().map(String::as_str).collect();
    if symbols.bare {
        names.push("an unnamed reference");
    }
    names.join(", ")
}

/// Violations of one rule reach the result together: `CheckRun::check_rules`
/// collects one `CheckResult` per rule and the `FromIterator` impl appends them
/// in that order, so grouping consecutive equals is enough to gather them. That
/// two rules cannot share a header rests on `check_unique_rule_names`, which
/// rejects a configuration whose rules repeat a name.
fn same_rule(a: &Violation, b: &Violation) -> bool {
    a.rule_name == b.rule_name && a.rule_type == b.rule_type && a.severity == b.severity
}

/// Render one rule's block: `{level}[rule-type]: rule-name` once, then every
/// violation of that rule below it. `level` is `error` or `warning`, taken
/// from the rule's severity, unless the block holds no reported violation, in
/// which case it is `silenced`.
fn rule_block(out: &mut String, violations: &[&Violation], level: &str) {
    let _ = writeln!(
        out,
        "{level}[{}]: {}",
        violations[0].rule_type, violations[0].rule_name
    );
    for violation in violations {
        violation_body(out, violation);
    }
    let _ = writeln!(out);
}

/// One violation under its rule header: the fact the rule established, then
/// the source locations that carry it. A silenced violation carries its state
/// on the line; a reported one carries no mark.
fn violation_body(out: &mut String, violation: &Violation) {
    let mark = state_mark(violation.state);
    match &violation.detail {
        ViolationDetail::Edge {
            edge: ends,
            frozen_for,
        } => {
            let _ = writeln!(out, "  = {}{mark}", edge(ends));
            if let Some(frozen_for) = frozen_for {
                let carries = EdgeSymbols::from_locations(&violation.locations);
                let _ = writeln!(
                    out,
                    "    frozen for {}; now also carries {}",
                    symbol_list(frozen_for),
                    symbol_list(&carries.difference(frozen_for))
                );
            }
        }
        ViolationDetail::Cluster(cluster) => {
            out.push_str(&cluster_block(cluster, "    ", mark));
        }
    }
    for loc in &violation.locations {
        let _ = writeln!(out, "    --> {}:{}", loc.file.display(), loc.line);
    }
}

fn state_mark(state: ViolationState) -> &'static str {
    match state {
        ViolationState::Reported => "",
        ViolationState::Allowed => " (allowed)",
        ViolationState::Frozen => " (frozen)",
    }
}

/// Render one cluster: header, then either a single-cycle body (the cycle plus
/// the edge carrying the fewest symbols) or a tangle body (the ranked feedback
/// edges). `indent` prefixes the header; the body indents further relative to
/// it, as before. `mark` is the silenced-state suffix from [`state_mark`],
/// placed on the position rather than after the counts, so it does not read
/// as part of them.
fn cluster_block(cluster: &CycleCluster, indent: &str, mark: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{indent}tangle {}/{}{mark}: {} ({}, {})",
        cluster.position,
        cluster.total,
        cluster.place,
        plural(cluster.modules, "module"),
        plural(cluster.cycles, "cycle"),
    );

    if let Some(names) = &cluster.cycle {
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
                writeln!(
                    out,
                    "{indent}  every circular dependency contains this edge"
                )
            } else {
                writeln!(
                    out,
                    "{indent}  every circular dependency contains at least one of these {count} edges"
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
            cycle: Some(vec![from.into(), to.into()]),
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
                state: ViolationState::Reported,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("domain::service", "infra::db"),
                    frozen_for: None,
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
                state: ViolationState::Reported,
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
                state: ViolationState::Reported,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("a", "b"),
                    frozen_for: None,
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

    /// Edge violation of `no infra in domain`, one location per call.
    fn domain_violation(from: &str, file: &str) -> Violation {
        Violation {
            rule_name: "no infra in domain".into(),
            rule_type: "forbidden-dependency".into(),
            severity: Severity::Error,
            state: ViolationState::Reported,
            detail: ViolationDetail::Edge {
                edge: Edge::new(from, "infra::db"),
                frozen_for: None,
            },
            locations: vec![SourceLocation {
                file: PathBuf::from(file),
                line: 12,
                symbols: vec![],
                module_path: String::new(),
                via_reexport: false,
            }],
        }
    }

    #[test]
    fn test_format_heads_a_rule_once_however_many_violations() {
        let result = CheckResult {
            violations: vec![
                domain_violation("domain::a", "src/domain/a.rs"),
                domain_violation("domain::b", "src/domain/b.rs"),
            ],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert_eq!(
            output
                .matches("error[forbidden-dependency]: no infra in domain")
                .count(),
            1,
            "got:\n{output}"
        );
        assert!(
            output.contains("  = domain::a → infra::db\n    --> src/domain/a.rs:12\n"),
            "got:\n{output}"
        );
        assert!(
            output.contains("  = domain::b → infra::db\n    --> src/domain/b.rs:12\n"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_heads_a_rule_once_when_part_of_it_is_silenced() {
        let result = CheckResult {
            violations: vec![
                domain_violation("domain::a", "src/domain/a.rs"),
                Violation {
                    state: ViolationState::Frozen,
                    ..domain_violation("domain::b", "src/domain/b.rs")
                },
            ],
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert_eq!(
            output
                .matches("error[forbidden-dependency]: no infra in domain")
                .count(),
            1,
            "got:\n{output}"
        );
        assert!(!output.contains("baseline["), "got:\n{output}");
        assert!(output.contains("domain::a → infra::db"), "got:\n{output}");
        assert!(output.contains("domain::b → infra::db"), "got:\n{output}");
    }

    /// The case the ticket measured: on wgpu one `no-cycles` rule repeated its
    /// header over twelve tangles.
    #[test]
    fn test_format_heads_a_rule_once_over_several_tangles() {
        let tangle = |position, from: &str, to: &str| Violation {
            rule_name: "no cycles".into(),
            rule_type: "no-cycles".into(),
            severity: Severity::Error,
            state: ViolationState::Reported,
            detail: ViolationDetail::Cluster(CycleCluster {
                position,
                total: 2,
                ..cluster_fixture(from, to)
            }),
            locations: vec![],
        };
        let result = CheckResult {
            violations: vec![tangle(1, "a", "b"), tangle(2, "c", "d")],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert_eq!(
            output.matches("error[no-cycles]: no cycles").count(),
            1,
            "got:\n{output}"
        );
        assert!(output.contains("tangle 1/2:"), "got:\n{output}");
        assert!(output.contains("tangle 2/2:"), "got:\n{output}");
    }

    #[test]
    fn test_format_heads_each_rule_separately() {
        let result = CheckResult {
            violations: vec![
                domain_violation("domain::a", "src/domain/a.rs"),
                Violation {
                    rule_name: "no cycles in domain".into(),
                    rule_type: "no-cycles".into(),
                    ..domain_violation("domain::b", "src/domain/b.rs")
                },
            ],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("error[forbidden-dependency]: no infra in domain"),
            "got:\n{output}"
        );
        assert!(
            output.contains("error[no-cycles]: no cycles in domain"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_status_lines_count_per_rule() {
        let result = CheckResult {
            checked_rules: vec![
                "rule1".into(),
                "rule2".into(),
                "rule3".into(),
                "rule4".into(),
            ],
            violations: vec![
                Violation {
                    rule_name: "rule1".into(),
                    rule_type: "forbidden-dependency".into(),
                    severity: Severity::Error,
                    state: ViolationState::Reported,
                    detail: ViolationDetail::Edge {
                        edge: Edge::new("a", "b"),
                        frozen_for: None,
                    },
                    locations: vec![],
                },
                Violation {
                    rule_name: "rule2".into(),
                    rule_type: "no-cycles".into(),
                    severity: Severity::Warn,
                    state: ViolationState::Reported,
                    detail: ViolationDetail::Cluster(cluster_fixture("c", "d")),
                    locations: vec![],
                },
                Violation {
                    rule_name: "rule3".into(),
                    rule_type: "layers".into(),
                    severity: Severity::Error,
                    state: ViolationState::Reported,
                    detail: ViolationDetail::Edge {
                        edge: Edge::new("x", "y"),
                        frozen_for: None,
                    },
                    locations: vec![],
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            format_status(&result),
            "rule1 FAILED: 1 errors, 0 warnings, 0 allowed, 0 frozen\n\
             rule2 WARN: 0 errors, 1 warnings, 0 allowed, 0 frozen\n\
             rule3 FAILED: 1 errors, 0 warnings, 0 allowed, 0 frozen\n\
             rule4 ok: 0 errors, 0 warnings, 0 allowed, 0 frozen\n\
             config ok: 0 errors, 0 warnings\n"
        );
    }

    #[test]
    fn test_format_empty() {
        let result = CheckResult::default();
        let output = format_violations(&result, false);
        assert!(output.is_empty());
    }

    fn allowed_edge_violation() -> Violation {
        Violation {
            rule_name: "no infra in domain".into(),
            rule_type: "forbidden-dependency".into(),
            severity: Severity::Error,
            state: ViolationState::Allowed,
            detail: ViolationDetail::Edge {
                edge: Edge::new("domain::service", "infra::db"),
                frozen_for: None,
            },
            locations: vec![],
        }
    }

    #[test]
    fn test_format_allowed_hidden_by_default_but_counted() {
        let result = CheckResult {
            violations: vec![allowed_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            !output.contains("except[forbidden-dependency]"),
            "got:\n{output}"
        );
        assert!(
            output.contains("1 violation allowed by except, not counted"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_allowed_shown_with_flag() {
        let result = CheckResult {
            violations: vec![allowed_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert!(
            output.contains("silenced[forbidden-dependency]: no infra in domain"),
            "got:\n{output}"
        );
        assert!(
            output.contains("= domain::service → infra::db"),
            "got:\n{output}"
        );
        assert!(!output.contains("not counted"), "got:\n{output}");
    }

    #[test]
    fn test_format_no_allowed_output_unchanged() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "no infra in domain".into(),
                rule_type: "forbidden-dependency".into(),
                severity: Severity::Error,
                state: ViolationState::Reported,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("domain::service", "infra::db"),
                    frozen_for: None,
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

    fn frozen_edge_violation() -> Violation {
        Violation {
            rule_name: "no storage in domain".into(),
            rule_type: "forbidden-dependency".into(),
            severity: Severity::Error,
            state: ViolationState::Frozen,
            detail: ViolationDetail::Edge {
                edge: Edge::new("domain::a", "storage::pool"),
                frozen_for: None,
            },
            locations: vec![],
        }
    }

    /// Two edges frozen under one rule.
    fn frozen_edge_pair() -> Vec<Violation> {
        vec![
            frozen_edge_violation(),
            Violation {
                detail: ViolationDetail::Edge {
                    edge: Edge::new("domain::b", "storage::pool"),
                    frozen_for: None,
                },
                ..frozen_edge_violation()
            },
        ]
    }

    #[test]
    fn test_format_counts_a_frozen_tangle_once() {
        // A frozen tangle is one cluster, not one violation per edge.
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "no cycles in domain".into(),
                rule_type: "no-cycles".into(),
                severity: Severity::Error,
                state: ViolationState::Frozen,
                detail: ViolationDetail::Cluster(cluster_fixture("a", "b")),
                locations: vec![],
            }],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("1 violation frozen in the baseline, not counted"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_frozen_hidden_by_default_but_counted() {
        let result = CheckResult {
            violations: vec![frozen_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(!output.contains("no storage in domain"), "got:\n{output}");
        assert!(
            output.contains("1 violation frozen in the baseline, not counted"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_frozen_shown_with_flag() {
        let result = CheckResult {
            violations: vec![allowed_edge_violation(), frozen_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert!(
            output.contains("silenced[forbidden-dependency]: no infra in domain"),
            "got:\n{output}"
        );
        assert!(
            output.contains("silenced[forbidden-dependency]: no storage in domain"),
            "got:\n{output}"
        );
        assert!(
            output.contains("= domain::a → storage::pool"),
            "got:\n{output}"
        );
        assert!(!output.contains("not counted"), "got:\n{output}");
    }

    #[test]
    fn test_format_heads_silenced_violations_once_per_rule() {
        let result = CheckResult {
            violations: frozen_edge_pair(),
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert_eq!(
            output
                .matches("silenced[forbidden-dependency]: no storage in domain")
                .count(),
            1,
            "got:\n{output}"
        );
        assert!(
            output.contains("  = domain::a → storage::pool"),
            "got:\n{output}"
        );
        assert!(
            output.contains("  = domain::b → storage::pool"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_heads_a_wholly_silenced_rule_with_silenced() {
        let result = CheckResult {
            violations: vec![frozen_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert!(
            output.contains("silenced[forbidden-dependency]: no storage in domain"),
            "got:\n{output}"
        );
        assert!(!output.contains("error["), "got:\n{output}");
        assert!(!output.contains("warning["), "got:\n{output}");
    }

    #[test]
    fn test_format_marks_a_silenced_entry_in_its_line() {
        let result = CheckResult {
            violations: vec![
                domain_violation("domain::a", "src/domain/a.rs"),
                Violation {
                    state: ViolationState::Allowed,
                    ..domain_violation("domain::b", "src/domain/b.rs")
                },
                Violation {
                    state: ViolationState::Frozen,
                    ..domain_violation("domain::c", "src/domain/c.rs")
                },
            ],
            ..Default::default()
        };
        let output = format_violations(&result, true);
        assert!(
            output.contains("domain::c → infra::db (frozen)"),
            "got:\n{output}"
        );
        assert!(
            output.contains("domain::b → infra::db (allowed)"),
            "got:\n{output}"
        );
        assert!(output.contains("domain::a → infra::db\n"), "got:\n{output}");
    }

    #[test]
    fn test_format_both_silenced_counts_combined() {
        let result = CheckResult {
            violations: vec![allowed_edge_violation(), frozen_edge_violation()],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output
                .contains("2 violations silenced (1 allowed by except, 1 frozen in the baseline)"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_only_frozen_no_reported_violations() {
        let result = CheckResult {
            violations: vec![frozen_edge_violation()],
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
    fn test_format_only_allowed_no_reported_violations() {
        let result = CheckResult {
            violations: vec![allowed_edge_violation()],
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

    use crate::rules::baseline::{BaselineEntry, ViolationKey};
    use crate::rules::config::DiagnosticLevel;
    use crate::rules::diagnostics::{DeadExcept, DeadPattern, Diagnostic, DiagnosticKind};

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
    fn test_format_unmatched_pattern_names_its_rule_and_the_consequence() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Deny,
                kind: DiagnosticKind::UnmatchedPattern {
                    entry: DeadPattern {
                        rule: "no infra in domain".into(),
                        pattern: "domian::**".into(),
                    },
                },
            }],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("  unmatched-pattern: domian::**"),
            "got:\n{output}"
        );
        assert!(
            output.contains(
                "    in rule \"no infra in domain\", matches no module: the rule checks nothing"
            ),
            "got:\n{output}"
        );
    }

    /// Typos of one rule collapse onto one line, so a rules file with several
    /// of them stays readable.
    #[test]
    fn test_format_unmatched_patterns_of_one_rule_share_one_line() {
        let dead = |pattern: &str| Diagnostic {
            level: DiagnosticLevel::Deny,
            kind: DiagnosticKind::UnmatchedPattern {
                entry: DeadPattern {
                    rule: "architecture layers".into(),
                    pattern: pattern.into(),
                },
            },
        };
        let result = CheckResult {
            diagnostics: vec![dead("domian"), dead("crate::infra")],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("  unmatched-pattern: domian, crate::infra"),
            "got:\n{output}"
        );
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

    fn baseline_entry(rule: &str, from: &str, to: &str, symbols: &[&str]) -> BaselineEntry {
        BaselineEntry {
            rule: rule.into(),
            key: ViolationKey {
                edge: Edge::new(from, to),
                symbols: EdgeSymbols {
                    named: symbols.iter().map(|s| (*s).to_string()).collect(),
                    bare: false,
                },
            },
        }
    }

    #[test]
    fn test_format_unmatched_baseline_entry_shows_the_violation() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warn,
                kind: DiagnosticKind::UnmatchedBaselineEntry {
                    entry: baseline_entry(
                        "no infra in domain",
                        "domain::service",
                        "infra::db",
                        &[],
                    ),
                },
            }],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("domain::service → infra::db"),
            "got:\n{output}"
        );
        assert!(output.contains("--generate-baseline"), "got:\n{output}");
    }

    #[test]
    fn test_format_wide_baseline_entry_names_what_no_longer_crosses() {
        let result = CheckResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warn,
                kind: DiagnosticKind::WideBaselineEntry {
                    entry: baseline_entry(
                        "core acyclic",
                        "core::writer",
                        "core::keywords",
                        &["RESERVED", "TYPES"],
                    ),
                    surplus: EdgeSymbols {
                        named: ["TYPES".to_string()].into_iter().collect(),
                        bare: false,
                    },
                },
            }],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("core::writer → core::keywords (TYPES no longer crosses)"),
            "got:\n{output}"
        );
        assert!(
            output.contains("freezes more than the edge carries"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_format_both_stale_entry_kinds_report_under_one_name() {
        let result = CheckResult {
            diagnostics: vec![
                Diagnostic {
                    level: DiagnosticLevel::Warn,
                    kind: DiagnosticKind::UnmatchedBaselineEntry {
                        entry: baseline_entry("a rule", "a", "b", &[]),
                    },
                },
                Diagnostic {
                    level: DiagnosticLevel::Warn,
                    kind: DiagnosticKind::WideBaselineEntry {
                        entry: baseline_entry("a rule", "c", "d", &["One"]),
                        surplus: EdgeSymbols {
                            named: ["One".to_string()].into_iter().collect(),
                            bare: false,
                        },
                    },
                },
            ],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert_eq!(
            output.matches("unmatched-baseline-entry").count(),
            2,
            "one line each, both under the same name; got:\n{output}"
        );
    }

    #[test]
    fn test_format_an_outgrown_edge_says_what_it_froze_and_what_it_added() {
        let result = CheckResult {
            violations: vec![Violation {
                rule_name: "core acyclic".into(),
                rule_type: "no-cycles".into(),
                severity: Severity::Error,
                state: ViolationState::Reported,
                detail: ViolationDetail::Edge {
                    edge: Edge::new("core::writer", "core::keywords"),
                    frozen_for: Some(EdgeSymbols {
                        named: ["RESERVED".to_string(), "TYPES".to_string()]
                            .into_iter()
                            .collect(),
                        bare: false,
                    }),
                },
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/writer.rs"),
                    line: 12,
                    symbols: vec![
                        "RESERVED".to_string(),
                        "TYPES".to_string(),
                        "MODIFIERS".to_string(),
                    ],
                    module_path: String::new(),
                    via_reexport: false,
                }],
            }],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(
            output.contains("frozen for RESERVED, TYPES; now also carries MODIFIERS"),
            "got:\n{output}"
        );
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
    fn test_diagnostics_count_on_the_config_line_not_against_a_rule() {
        let result = CheckResult {
            checked_rules: vec!["some rule".into()],
            diagnostics: vec![unlayered("xtask", DiagnosticLevel::Deny)],
            ..Default::default()
        };
        assert_eq!(
            format_status(&result),
            "some rule ok: 0 errors, 0 warnings, 0 allowed, 0 frozen\n\
             config FAILED: 1 errors, 0 warnings\n"
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
            cycle: Some(vec!["a".into(), "b".into()]),
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
                state: ViolationState::Reported,
                detail: ViolationDetail::Cluster(cluster),
                locations: vec![],
            }],
            ..Default::default()
        };
        let output = format_violations(&result, false);
        assert!(output.contains("error[no-cycles]: no cycles in domain"));
        assert!(output.contains("  tangle 1/1: app (2 modules, 1 cycle)"));
        assert!(output.contains("    cycle: a -> b -> a"));
    }

    use crate::graph::{ArcGraph, EdgeWeight, Node, Reexports};

    // ===== cluster_block tests =====

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
                g.add_edge(crate_idx, n, EdgeWeight::Contains);
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
                EdgeWeight::ModuleDep {
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
        let out = cluster_block(&clusters[0], "", "");
        assert!(
            out.contains("tangle 1/1: app (2 modules, 1 cycle)"),
            "got:\n{out}"
        );
        assert!(out.contains("cycle: a -> b -> a"), "got:\n{out}");
        assert!(
            out.contains("fewest symbols: a -> b (1 symbol)"),
            "got:\n{out}"
        );
    }

    #[test]
    fn cluster_report_tangle_block() {
        // Two triangles share a->b, plus a separate a<->e cycle on the same
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
        let out = cluster_block(&clusters[0], "", "");
        assert!(out.contains("(5 modules, 3 cycles)"), "got:\n{out}");
        assert!(out.contains("edges, most cycles first:"), "got:\n{out}");
        assert!(out.contains("(on 2 cycles, 1 symbol)"), "got:\n{out}");
        assert!(
            out.contains("every circular dependency contains at least one of these 2 edges"),
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
        let out = cluster_block(&clusters[0], "", "");
        assert!(out.contains("  edges:"), "got:\n{out}");
        assert!(!out.contains("most cycles first"), "got:\n{out}");
        assert!(
            !out.contains("every circular dependency contains"),
            "got:\n{out}"
        );
    }

    #[test]
    fn cluster_report_closing_line_is_singular_for_a_lone_edge() {
        // Two triangles sharing edge a->b: two cycles, but one edge hits both.
        let g = cyc_graph(
            &["a", "b", "c", "d"],
            &[(0, 1, 1), (1, 2, 1), (2, 0, 1), (1, 3, 1), (3, 0, 1)],
        );
        let clusters = report_of(&g);
        let out = cluster_block(&clusters[0], "", "");
        assert!(
            out.contains("every circular dependency contains this edge"),
            "got:\n{out}"
        );
    }
}
