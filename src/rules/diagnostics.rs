//! Gaps in the configuration itself, as opposed to violations of it.
//!
//! A rule states something about the code, a diagnostic about the rules: a
//! crate no layer sorts, an entry that suppresses nothing any more. Each one
//! carries the level its `[diagnostics]` entry set.

use crate::rules::baseline::{Baseline, BaselineEntry};
use crate::rules::config::{ArcConfig, DiagnosticLevel, Rule, RuleKind, Severity};
use crate::rules::matching::PatternIndex;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;

#[derive(Debug)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub kind: DiagnosticKind,
}

#[derive(Debug)]
pub enum DiagnosticKind {
    /// A workspace crate that no `layers` rule sorts into a layer. `layers` is
    /// a total statement, so a crate missing from it is not allowed but
    /// unsorted: every edge touching it is skipped without a word.
    UnlayeredCrate { krate: String },
    /// A frozen finding the run no longer produces: fixed, or its rule renamed
    /// out from under the entry.
    UnmatchedBaselineEntry { entry: BaselineEntry },
    /// An `except` pattern that resolves to no module, so it allows nothing.
    UnmatchedExcept { entry: DeadExcept },
}

impl Diagnostic {
    /// The name this diagnostic is configured under in `[diagnostics]`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self.kind {
            DiagnosticKind::UnlayeredCrate { .. } => "unlayered-crate",
            DiagnosticKind::UnmatchedBaselineEntry { .. } => "unmatched-baseline-entry",
            DiagnosticKind::UnmatchedExcept { .. } => "unmatched-except",
        }
    }
}

/// Every gap the config asks to hear about. `hits` are the baseline entries
/// the run matched, so the rest of the baseline is stale.
#[must_use]
pub(super) fn collect(
    index: &PatternIndex,
    config: &ArcConfig,
    baseline: &Baseline,
    hits: &[BaselineEntry],
) -> Vec<Diagnostic> {
    let settings = &config.diagnostics;
    let mut found = Vec::new();

    let level = settings.unlayered_crate.level;
    if level != DiagnosticLevel::Allow {
        found.extend(
            unlayered_crates(index, config)
                .into_iter()
                .map(|krate| Diagnostic {
                    level,
                    kind: DiagnosticKind::UnlayeredCrate { krate },
                }),
        );
    }

    let level = settings.unmatched_baseline_entry;
    if level != DiagnosticLevel::Allow {
        found.extend(
            baseline
                .unmatched(hits)
                .into_iter()
                .filter(|entry| !is_ignored(config, &entry.rule))
                .map(|entry| Diagnostic {
                    level,
                    kind: DiagnosticKind::UnmatchedBaselineEntry { entry },
                }),
        );
    }

    let level = settings.unmatched_except;
    if level != DiagnosticLevel::Allow {
        found.extend(
            dead_excepts(index, config)
                .into_iter()
                .map(|entry| Diagnostic {
                    level,
                    kind: DiagnosticKind::UnmatchedExcept { entry },
                }),
        );
    }

    found
}

/// Rules the run evaluates; `Severity::Ignore` switches a rule off entirely,
/// and a rule that is not checked makes no statement to have a gap in.
fn active_rules(config: &ArcConfig) -> impl Iterator<Item = &Rule> {
    config
        .rules
        .iter()
        .filter(|rule| rule.severity() != Severity::Ignore)
}

fn is_ignored(config: &ArcConfig, rule_name: &str) -> bool {
    config
        .rules
        .iter()
        .any(|rule| rule.name == rule_name && rule.severity() == Severity::Ignore)
}

/// Workspace crates that no layer pattern of any active `layers` rule reaches,
/// minus the ones the config puts outside on purpose.
///
/// The layer patterns of all rules are taken together: a crate layered by one
/// rule is sorted, and reporting it against a second rule that says nothing
/// about it would make module-level layering unusable.
fn unlayered_crates(index: &PatternIndex, config: &ArcConfig) -> Vec<String> {
    let layer_patterns: Vec<&str> = active_rules(config)
        .filter_map(|rule| match &rule.kind {
            RuleKind::Layers(params) => Some(&params.layers),
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .collect();
    if layer_patterns.is_empty() {
        return Vec::new();
    }

    let graph = index.graph();
    let layered: HashSet<NodeIndex> = layer_patterns
        .into_iter()
        .flat_map(|pattern| index.resolve(pattern))
        .map(|idx| graph.owning_crate(idx))
        .collect();

    let except = &config.diagnostics.unlayered_crate.except;
    let mut crates: Vec<String> = graph
        .node_indices()
        .filter(|&idx| graph[idx].is_crate() && !layered.contains(&idx))
        .map(|idx| graph[idx].name().to_string())
        .filter(|name| !except.contains(name))
        .collect();
    crates.sort();
    crates
}

/// An `except` pattern that matches no module: a typo or a rename, and it
/// silently allows nothing.
#[derive(Debug)]
pub struct DeadExcept {
    pub rule: String,
    pub pattern: String,
}

/// `except` patterns across `config` whose `from` or `to` side resolves to no
/// node. An `except` on a currently nonexistent *edge* is not dead — that's a
/// forward-looking allowance, not a typo — so only the pattern side is
/// checked, never whether the edge itself exists.
#[must_use]
pub(super) fn dead_excepts(index: &PatternIndex, config: &ArcConfig) -> Vec<DeadExcept> {
    let mut dead = Vec::new();
    for rule in active_rules(config) {
        for exception in &rule.except {
            if index.resolve(&exception.from).is_empty() {
                dead.push(DeadExcept {
                    rule: rule.name.clone(),
                    pattern: exception.from.clone(),
                });
            }
            if index.resolve(&exception.to).is_empty() {
                dead.push(DeadExcept {
                    rule: rule.name.clone(),
                    pattern: exception.to.clone(),
                });
            }
        }
    }
    dead
}

#[cfg(test)]
mod tests {
    use crate::graph::{ArcGraph, Edge, Node};
    use crate::rules::baseline::{Baseline, BaselineEntry, FindingKey};
    use crate::rules::config::{
        ArcConfig, DiagnosticLevel, Diagnostics, Direction, Except, ForbiddenDependencyRule,
        LayersRule, Rule, RuleKind, Severity, UnlayeredCrate,
    };
    use crate::rules::diagnostics::{Diagnostic, DiagnosticKind, collect, dead_excepts};
    use crate::rules::matching::PatternIndex;
    use std::path::PathBuf;

    /// Workspace graph with one module per named crate, so that a crate
    /// pattern and a module pattern both have something to resolve to.
    fn workspace(crates: &[&str]) -> ArcGraph {
        let mut graph = ArcGraph::new();
        for name in crates {
            let crate_idx = graph.add_node(Node::Crate {
                name: (*name).into(),
                path: PathBuf::from(format!("/{name}")),
            });
            let module = graph.add_node(Node::Module {
                name: "service".into(),
                crate_idx,
            });
            graph.add_edge(crate_idx, module, Edge::Contains);
        }
        graph
    }

    fn layers_rule(name: &str, layers: &[&str]) -> Rule {
        Rule {
            name: name.into(),
            declared_severity: Some(Severity::Error),
            except: vec![],
            kind: RuleKind::Layers(LayersRule {
                layers: layers.iter().map(|&layer| layer.into()).collect(),
                direction: Direction::TopDown,
            }),
        }
    }

    fn forbidden_rule(name: &str, except: Vec<Except>) -> Rule {
        Rule {
            name: name.into(),
            declared_severity: Some(Severity::Error),
            except,
            kind: RuleKind::ForbiddenDependency(ForbiddenDependencyRule {
                from: "domain::**".into(),
                to: "infra::**".into(),
            }),
        }
    }

    fn except_edge(from: &str, to: &str) -> Except {
        Except {
            from: from.into(),
            to: to.into(),
            reason: None,
        }
    }

    fn config_of(rules: Vec<Rule>, diagnostics: Diagnostics) -> ArcConfig {
        ArcConfig {
            config: None,
            rules,
            diagnostics,
        }
    }

    fn unlayered_except(crates: &[&str]) -> Diagnostics {
        Diagnostics {
            unlayered_crate: UnlayeredCrate {
                level: DiagnosticLevel::Warn,
                except: crates.iter().map(|&name| name.into()).collect(),
            },
            ..Diagnostics::default()
        }
    }

    /// `collect` against an empty baseline.
    fn diagnose(graph: &ArcGraph, config: &ArcConfig) -> Vec<Diagnostic> {
        collect(&PatternIndex::build(graph), config, &Baseline::empty(), &[])
    }

    /// `collect` against a baseline holding `entries`, of which `hits` were
    /// matched by the run.
    fn diagnose_baseline(
        graph: &ArcGraph,
        config: &ArcConfig,
        entries: &[BaselineEntry],
        hits: &[BaselineEntry],
    ) -> Vec<Diagnostic> {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("arc-baseline.toml");
        Baseline::write(&path, entries).unwrap();
        let baseline = Baseline::load(&path).unwrap();
        collect(&PatternIndex::build(graph), config, &baseline, hits)
    }

    fn unlayered(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::UnlayeredCrate { krate } => Some(krate.as_str()),
                _ => None,
            })
            .collect()
    }

    fn unmatched_entries(diagnostics: &[Diagnostic]) -> Vec<&BaselineEntry> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::UnmatchedBaselineEntry { entry } => Some(entry),
                _ => None,
            })
            .collect()
    }

    // ===== unlayered-crate =====

    #[test]
    fn crate_outside_every_layer_pattern_is_reported_once() {
        let graph = workspace(&["domain", "infra", "xtask"]);
        let config = config_of(
            vec![layers_rule("architecture layers", &["infra", "domain"])],
            Diagnostics::default(),
        );
        assert_eq!(unlayered(&diagnose(&graph, &config)), ["xtask"]);
    }

    #[test]
    fn crate_on_the_except_list_is_not_reported() {
        let graph = workspace(&["domain", "infra", "xtask"]);
        let config = config_of(
            vec![layers_rule("architecture layers", &["infra", "domain"])],
            unlayered_except(&["xtask"]),
        );
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn without_a_layers_rule_no_crate_is_unlayered() {
        // A config that never claims a layering leaves no gap: silence there
        // is its own statement, not an omission.
        let graph = workspace(&["domain", "xtask"]);
        let config = config_of(vec![], Diagnostics::default());
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn a_crate_layered_by_one_of_two_rules_is_not_reported() {
        let graph = workspace(&["domain", "infra", "tools"]);
        let config = config_of(
            vec![
                layers_rule("core layers", &["infra", "domain"]),
                layers_rule("tool layers", &["tools"]),
            ],
            Diagnostics::default(),
        );
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn a_module_pattern_layers_the_crate_it_sits_in() {
        // Layering inside one crate: that crate takes part, the others do not.
        let graph = workspace(&["app", "xtask"]);
        let config = config_of(
            vec![layers_rule("app layers", &["app::service"])],
            Diagnostics::default(),
        );
        assert_eq!(unlayered(&diagnose(&graph, &config)), ["xtask"]);
    }

    #[test]
    fn a_layers_rule_set_to_ignore_makes_no_claim() {
        let graph = workspace(&["domain", "xtask"]);
        let mut rule = layers_rule("architecture layers", &["domain"]);
        rule.declared_severity = Some(Severity::Ignore);
        let config = config_of(vec![rule], Diagnostics::default());
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn allow_silences_a_diagnostic() {
        let graph = workspace(&["domain", "xtask"]);
        let config = config_of(
            vec![layers_rule("architecture layers", &["domain"])],
            Diagnostics {
                unlayered_crate: UnlayeredCrate {
                    level: DiagnosticLevel::Allow,
                    except: vec![],
                },
                ..Diagnostics::default()
            },
        );
        assert!(diagnose(&graph, &config).is_empty());
    }

    #[test]
    fn the_configured_level_reaches_the_diagnostic() {
        let graph = workspace(&["domain", "xtask"]);
        let config = config_of(
            vec![layers_rule("architecture layers", &["domain"])],
            Diagnostics {
                unlayered_crate: UnlayeredCrate {
                    level: DiagnosticLevel::Deny,
                    except: vec![],
                },
                ..Diagnostics::default()
            },
        );
        let found = diagnose(&graph, &config);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].level, DiagnosticLevel::Deny);
    }

    // ===== unmatched-except =====

    #[test]
    fn except_pattern_matching_no_module_is_a_dead_entry() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule(
                "no infra in domain",
                vec![except_edge("domain::typo", "infra::service")],
            )],
            Diagnostics::default(),
        );
        let dead = dead_excepts(&PatternIndex::build(&graph), &config);
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].rule, "no infra in domain");
        assert_eq!(dead[0].pattern, "domain::typo");
    }

    #[test]
    fn a_resolving_except_is_not_dead() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule(
                "no infra in domain",
                vec![except_edge("domain::service", "infra::service")],
            )],
            Diagnostics::default(),
        );
        assert!(dead_excepts(&PatternIndex::build(&graph), &config).is_empty());
    }

    #[test]
    fn dead_except_is_reported_as_a_diagnostic() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule(
                "no infra in domain",
                vec![except_edge("domain::typo", "infra::service")],
            )],
            Diagnostics::default(),
        );
        let found = diagnose(&graph, &config);
        assert!(
            found.iter().any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::UnmatchedExcept { entry }
                    if entry.rule == "no infra in domain" && entry.pattern == "domain::typo"
            )),
            "got: {found:?}"
        );
    }

    // ===== unmatched-baseline-entry =====

    fn stale_entry(rule: &str) -> BaselineEntry {
        BaselineEntry {
            rule: rule.into(),
            key: FindingKey::edge("domain::service", "infra::service"),
        }
    }

    #[test]
    fn baseline_entry_no_finding_matched_is_reported() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule("no infra in domain", vec![])],
            Diagnostics::default(),
        );
        let stale = stale_entry("no infra in domain");
        let found = diagnose_baseline(&graph, &config, std::slice::from_ref(&stale), &[]);
        assert_eq!(unmatched_entries(&found), [&stale]);
    }

    #[test]
    fn baseline_entry_of_an_ignored_rule_is_not_reported() {
        // `severity = "ignore"` says the rule is not checked; reporting its
        // frozen findings as stale would turn that statement around.
        let graph = workspace(&["domain", "infra"]);
        let mut rule = forbidden_rule("no infra in domain", vec![]);
        rule.declared_severity = Some(Severity::Ignore);
        let config = config_of(vec![rule], Diagnostics::default());
        let found = diagnose_baseline(&graph, &config, &[stale_entry("no infra in domain")], &[]);
        assert!(unmatched_entries(&found).is_empty());
    }

    #[test]
    fn baseline_entry_of_a_renamed_rule_is_reported() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule("the new name", vec![])],
            Diagnostics::default(),
        );
        let orphan = stale_entry("the old name");
        let found = diagnose_baseline(&graph, &config, std::slice::from_ref(&orphan), &[]);
        assert_eq!(unmatched_entries(&found), [&orphan]);
    }

    #[test]
    fn a_hit_entry_is_not_reported() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule("no infra in domain", vec![])],
            Diagnostics::default(),
        );
        let frozen = [stale_entry("no infra in domain")];
        let found = diagnose_baseline(&graph, &config, &frozen, &frozen);
        assert!(unmatched_entries(&found).is_empty());
    }
}
