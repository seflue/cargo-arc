//! Gaps in the configuration itself, as opposed to violations of it.
//!
//! A rule states something about the code, a diagnostic about the rules: a
//! node an exhaustive rule leaves in no position, an entry that freezes
//! nothing any more. Each one carries the level its `[diagnostics]` entry set.

use crate::model::EdgeSymbols;
use crate::rules::baseline::{Baseline, BaselineEntry, StaleEntry};
use crate::rules::config::{ArcConfig, DiagnosticLevel, Layer, Rule, RuleKind, Severity};
use crate::rules::matching::{PatternIndex, ResolvedAllows};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub kind: DiagnosticKind,
}

#[derive(Debug)]
pub enum DiagnosticKind {
    /// A node an exhaustive `layers` rule leaves in no position. Only the topmost
    /// node of a containment chain is reported: a missing entry is one gap, not its
    /// whole subtree. Every edge touching the node is skipped without a word.
    UnlayeredNode { entry: UnsortedNode },
    /// A frozen violation the run no longer produces: fixed, or its rule renamed
    /// out from under the entry.
    UnmatchedBaselineEntry { entry: BaselineEntry },
    /// A frozen edge that still violates, but no longer carries every symbol its
    /// entry tolerates. `surplus` is the part nothing crosses any more.
    WideBaselineEntry {
        entry: BaselineEntry,
        surplus: EdgeSymbols,
    },
    /// An `allow` pattern that resolves to no module, so it allows nothing.
    UnmatchedAllow { entry: DeadAllow },
    /// `allow` entries of one rule that put nodes above each other in a
    /// circle, so together they declare no order.
    ContradictoryAllow { entry: ContradictoryAllow },
    /// A rule's own pattern that resolves to no module, so the rule checks
    /// nothing and reports nothing.
    UnmatchedPattern { entry: DeadPattern },
    /// A rule's catch-all layer whose rest is empty: its ordinary layers
    /// already cover every node, so `*` never receives one.
    DeadCatchAllLayer { rule: String },
}

impl Diagnostic {
    /// The name this diagnostic is configured under in `[diagnostics]`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self.kind {
            DiagnosticKind::UnlayeredNode { .. } => "unlayered-node",
            // Both say the baseline names something the run does not confirm;
            // the config has one switch for the pair.
            DiagnosticKind::UnmatchedBaselineEntry { .. }
            | DiagnosticKind::WideBaselineEntry { .. } => "unmatched-baseline-entry",
            DiagnosticKind::UnmatchedAllow { .. } => "unmatched-allow",
            DiagnosticKind::ContradictoryAllow { .. } => "contradictory-allow",
            DiagnosticKind::UnmatchedPattern { .. } | DiagnosticKind::DeadCatchAllLayer { .. } => {
                "unmatched-pattern"
            }
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

    let level = settings.unlayered_node.level;
    if level != DiagnosticLevel::Allow {
        found.extend(
            unlayered_nodes(index, config)
                .into_iter()
                .map(|entry| Diagnostic {
                    level,
                    kind: DiagnosticKind::UnlayeredNode { entry },
                }),
        );
    }

    let level = settings.unmatched_baseline_entry;
    if level != DiagnosticLevel::Allow {
        found.extend(
            baseline
                .unmatched(hits)
                .into_iter()
                .filter(|stale| !is_ignored(config, &stale.entry().rule))
                .map(|stale| Diagnostic {
                    level,
                    kind: match stale {
                        StaleEntry::Gone(entry) => DiagnosticKind::UnmatchedBaselineEntry { entry },
                        StaleEntry::TooWide { entry, surplus } => {
                            DiagnosticKind::WideBaselineEntry { entry, surplus }
                        }
                    },
                }),
        );
    }

    let level = settings.unmatched_allow;
    if level != DiagnosticLevel::Allow {
        found.extend(
            dead_allows(index, config)
                .into_iter()
                .map(|entry| Diagnostic {
                    level,
                    kind: DiagnosticKind::UnmatchedAllow { entry },
                }),
        );
    }

    let level = settings.contradictory_allow;
    if level != DiagnosticLevel::Allow {
        found.extend(
            contradictory_allows(index, config)
                .into_iter()
                .map(|entry| Diagnostic {
                    level,
                    kind: DiagnosticKind::ContradictoryAllow { entry },
                }),
        );
    }

    let level = settings.unmatched_pattern;
    if level != DiagnosticLevel::Allow {
        found.extend(
            unmatched_patterns(index, config)
                .into_iter()
                .map(|entry| Diagnostic {
                    level,
                    kind: DiagnosticKind::UnmatchedPattern { entry },
                }),
        );
        found.extend(
            dead_catch_alls(index, config)
                .into_iter()
                .map(|rule| Diagnostic {
                    level,
                    kind: DiagnosticKind::DeadCatchAllLayer { rule },
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
        .filter(|rule| rule.severity != Severity::Ignore)
}

fn is_ignored(config: &ArcConfig, rule_name: &str) -> bool {
    config
        .rules
        .iter()
        .any(|rule| rule.name == rule_name && rule.severity == Severity::Ignore)
}

/// A node an exhaustive `layers` rule leaves in no position.
#[derive(Debug)]
pub struct UnsortedNode {
    pub rule: String,
    pub node: String,
}

/// Nodes an exhaustive `layers` rule claims but does not sort into a
/// position, one rule at a time. A rule that does not declare itself
/// `exhaustive` makes no claim and contributes nothing here. A node the
/// `[diagnostics]` `except` list resolves to is dropped with its subtree, and
/// of a containment chain only the topmost unsorted node survives.
fn unlayered_nodes(index: &PatternIndex, config: &ArcConfig) -> Vec<UnsortedNode> {
    let graph = index.graph();
    let excepted: HashSet<NodeIndex> = config
        .diagnostics
        .unlayered_node
        .except
        .iter()
        .flat_map(|name| index.resolve(name))
        .collect();
    let parents = graph.parent_map();

    let mut found = Vec::new();
    for rule in active_rules(config) {
        let RuleKind::Layers(params) = &rule.kind else {
            continue;
        };
        if !params.exhaustive {
            continue;
        }

        let patterns: Vec<&str> = params
            .layers
            .iter()
            .filter_map(Layer::patterns)
            .flatten()
            .map(String::as_str)
            .collect();

        let sorted: HashSet<NodeIndex> = patterns
            .iter()
            .flat_map(|pattern| index.resolve(pattern))
            .collect();

        let mut claimed: HashSet<NodeIndex> = HashSet::new();
        if patterns.iter().any(|pattern| !pattern.contains("::")) {
            claimed.extend(graph.node_indices().filter(|&idx| graph[idx].is_crate()));
        }

        let touched: HashSet<NodeIndex> = patterns
            .iter()
            .filter(|pattern| pattern.contains("::"))
            .flat_map(|pattern| index.resolve(pattern))
            .map(|idx| graph.owning_crate(idx))
            .collect();
        if !touched.is_empty() {
            claimed.extend(graph.node_indices().filter(|&idx| {
                graph[idx].is_module() && touched.contains(&graph.owning_crate(idx))
            }));
        }

        let unsorted: HashSet<NodeIndex> = claimed
            .into_iter()
            .filter(|idx| !sorted.contains(idx) && !excepted.contains(idx))
            .collect();

        let mut names: Vec<String> = unsorted
            .iter()
            .filter(|idx| {
                parents
                    .get(idx)
                    .is_none_or(|parent| !unsorted.contains(parent))
            })
            .map(|&idx| graph.qualified_name(idx))
            .collect();
        names.sort();

        found.extend(names.into_iter().map(|node| UnsortedNode {
            rule: rule.name.clone(),
            node,
        }));
    }
    found
}

/// An `allow` pattern that matches no module: a typo or a rename, and it
/// silently allows nothing.
#[derive(Debug)]
pub struct DeadAllow {
    pub rule: String,
    pub pattern: String,
}

/// `allow` patterns across `config` whose `from` or `to` side resolves to no
/// node. An entry on a currently nonexistent *edge* is not dead — that's a
/// forward-looking allowance, not a typo — so only the pattern side is
/// checked, never whether the edge itself exists. A relative `to` has no
/// pattern of its own and is asked nothing.
#[must_use]
pub(super) fn dead_allows(index: &PatternIndex, config: &ArcConfig) -> Vec<DeadAllow> {
    let mut dead = Vec::new();
    for rule in active_rules(config) {
        for entry in &rule.allow {
            let sides = [Some(entry.from.as_str()), entry.to.pattern()];
            for pattern in sides.into_iter().flatten() {
                if index.resolve(pattern).is_empty() {
                    dead.push(DeadAllow {
                        rule: rule.name.clone(),
                        pattern: pattern.to_owned(),
                    });
                }
            }
        }
    }
    dead
}

/// `allow` entries of one rule whose declared order runs in a circle. Each
/// entry says its `to` stands above its `from`; `entries` are the ones that
/// take part in the circle, as written, and `cycle` is one circle of nodes
/// that shows it.
#[derive(Debug)]
pub struct ContradictoryAllow {
    pub rule: String,
    pub entries: Vec<String>,
    pub cycle: Vec<String>,
}

/// The circles in the order each rule's `allow` entries declare: a strongly
/// connected component of more than one node in the graph the entries lay
/// out over the nodes they cover.
fn contradictory_allows(index: &PatternIndex, config: &ArcConfig) -> Vec<ContradictoryAllow> {
    let graph = index.graph();
    let mut found = Vec::new();
    for rule in active_rules(config) {
        if rule.allow.is_empty() {
            continue;
        }
        let order = declared_order(&ResolvedAllows::resolve(&rule.allow, index));
        for component in tarjan_scc(&order) {
            if component.len() < 2 {
                continue;
            }
            let members: HashSet<NodeIndex> = component.iter().copied().collect();
            found.push(ContradictoryAllow {
                rule: rule.name.clone(),
                entries: entries_within(&order, &members)
                    .into_iter()
                    .map(|position| rule.allow[position].written())
                    .collect(),
                cycle: example_cycle(&order, &members)
                    .into_iter()
                    .map(|node| graph.qualified_name(order[node]))
                    .collect(),
            });
        }
    }
    found.sort_by(|a, b| a.rule.cmp(&b.rule).then_with(|| a.cycle.cmp(&b.cycle)));
    found
}

/// The order a rule's entries declare, as a graph from each covered `from`
/// node to its `to` node, the edge weighted with the entry's position. An
/// entry ranks a pair only where it covers one direction of it: a pair it
/// covers both ways it leaves unordered, and with the same pattern on both
/// sides that is every pair.
fn declared_order(allows: &ResolvedAllows) -> DiGraph<NodeIndex, usize> {
    let mut covered: HashMap<usize, HashSet<(NodeIndex, NodeIndex)>> = HashMap::new();
    for (source, target, position) in allows.pairs() {
        covered
            .entry(position)
            .or_default()
            .insert((source, target));
    }

    let mut order = DiGraph::new();
    let mut order_node: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    for (position, pairs) in covered {
        for &(source, target) in &pairs {
            if pairs.contains(&(target, source)) {
                continue;
            }
            let from = *order_node
                .entry(source)
                .or_insert_with(|| order.add_node(source));
            let to = *order_node
                .entry(target)
                .or_insert_with(|| order.add_node(target));
            order.add_edge(from, to, position);
        }
    }
    order
}

/// The positions of the entries whose edges run inside `members`, each once,
/// in the order the entries were written.
fn entries_within(order: &DiGraph<NodeIndex, usize>, members: &HashSet<NodeIndex>) -> Vec<usize> {
    let mut positions: Vec<usize> = order
        .edge_indices()
        .filter(|&edge| {
            let (from, to) = order.edge_endpoints(edge).expect("edge should exist");
            members.contains(&from) && members.contains(&to)
        })
        .map(|edge| order[edge])
        .collect();
    positions.sort_unstable();
    positions.dedup();
    positions
}

/// One circle inside `members`, as the nodes it runs through, closed by
/// repeating the first. Starts at the smallest name, so the choice does not
/// depend on the order the component was collected in, and follows the
/// smallest-named successor from there until a node repeats.
fn example_cycle(
    order: &DiGraph<NodeIndex, usize>,
    members: &HashSet<NodeIndex>,
) -> Vec<NodeIndex> {
    let start = members
        .iter()
        .copied()
        .min_by_key(|&node| order[node])
        .expect("a component holds at least one node");
    let mut path = vec![start];
    let mut node = start;
    loop {
        let next = order
            .neighbors(node)
            .filter(|next| members.contains(next))
            .min_by_key(|&next| order[next])
            .expect("every node of a strongly connected component has a successor in it");
        if let Some(at) = path.iter().position(|&seen| seen == next) {
            path.push(next);
            return path.split_off(at);
        }
        path.push(next);
        node = next;
    }
}

/// A rule pattern that matches no module: a typo or a rename, and the rule it
/// belongs to then checks nothing.
#[derive(Debug)]
pub struct DeadPattern {
    pub rule: String,
    pub pattern: String,
}

/// Patterns across `config` that resolve to no node, `allow` aside. A rule
/// whose pattern misses has no other way of saying so: it checks zero edges,
/// reports nothing, and leaves the run green.
fn unmatched_patterns(index: &PatternIndex, config: &ArcConfig) -> Vec<DeadPattern> {
    active_rules(config)
        .flat_map(|rule| {
            rule.kind
                .patterns()
                .into_iter()
                .filter(|pattern| index.resolve(pattern).is_empty())
                .map(|pattern| DeadPattern {
                    rule: rule.name.clone(),
                    pattern: pattern.to_owned(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Rules whose catch-all layer's rest is empty: the ordinary layers already
/// cover every node, the way Rust reports an unreachable `_` arm. Unlike
/// [`unmatched_patterns`], this is not a typo: the rule's other positions do
/// cover the workspace, and `*` is left with nothing to add.
fn dead_catch_alls(index: &PatternIndex, config: &ArcConfig) -> Vec<String> {
    active_rules(config)
        .filter_map(|rule| {
            let RuleKind::Layers(params) = &rule.kind else {
                return None;
            };
            let rest = index.layer_rest(&params.layers)?;
            rest.is_empty().then(|| rule.name.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::graph::{ArcGraph, EdgeWeight};
    use crate::model::{Edge, EdgeSymbols};
    use crate::rules::baseline::{Baseline, BaselineEntry, ViolationKey};
    use crate::rules::config::{
        AllowedEdge, ArcConfig, DiagnosticLevel, Diagnostics, Direction, ForbiddenDependencyRule,
        Layer, LayersRule, NoCyclesRule, Rule, RuleKind, Severity, UnlayeredNode,
    };
    use crate::rules::diagnostics::{
        ContradictoryAllow, Diagnostic, DiagnosticKind, collect, dead_allows,
    };
    use crate::rules::matching::PatternIndex;
    use crate::rules::test_support::allow_edge;
    use crate::test_support::{crate_node, module_node};
    use petgraph::graph::NodeIndex;

    /// Workspace graph with one module per named crate, so that a crate
    /// pattern and a module pattern both have something to resolve to.
    fn workspace(crates: &[&str]) -> ArcGraph {
        let mut graph = ArcGraph::new();
        for name in crates {
            let crate_idx = graph.add_node(crate_node(name));
            let module = graph.add_node(module_node("service", crate_idx));
            graph.add_edge(crate_idx, module, EdgeWeight::Contains);
        }
        graph
    }

    fn add_crate(graph: &mut ArcGraph, name: &str) -> NodeIndex {
        graph.add_node(crate_node(name))
    }

    fn add_module(
        graph: &mut ArcGraph,
        name: &str,
        crate_idx: NodeIndex,
        parent: NodeIndex,
    ) -> NodeIndex {
        let idx = graph.add_node(module_node(name, crate_idx));
        graph.add_edge(parent, idx, EdgeWeight::Contains);
        idx
    }

    fn layers_rule(name: &str, layers: &[&str]) -> Rule {
        Rule {
            name: name.into(),
            severity: Severity::Error,
            allow: vec![],
            kind: RuleKind::Layers(LayersRule {
                layers: layers.iter().map(|&layer| layer.into()).collect(),
                direction: Direction::TopDown,
                exhaustive: false,
            }),
        }
    }

    fn exhaustive_layers_rule(name: &str, layers: &[&str]) -> Rule {
        Rule {
            kind: RuleKind::Layers(LayersRule {
                layers: layers.iter().map(|&layer| layer.into()).collect(),
                direction: Direction::TopDown,
                exhaustive: true,
            }),
            ..layers_rule(name, layers)
        }
    }

    fn cycles_rule(name: &str, scope: &str) -> Rule {
        Rule {
            name: name.into(),
            severity: Severity::Error,
            allow: vec![],
            kind: RuleKind::NoCycles(NoCyclesRule {
                scope: scope.into(),
            }),
        }
    }

    fn forbidden_between(name: &str, from: &str, to: &str) -> Rule {
        Rule {
            name: name.into(),
            severity: Severity::Error,
            allow: vec![],
            kind: RuleKind::ForbiddenDependency(ForbiddenDependencyRule {
                from: from.into(),
                to: to.into(),
            }),
        }
    }

    fn forbidden_rule(name: &str, allow: Vec<AllowedEdge>) -> Rule {
        Rule {
            allow,
            ..forbidden_between(name, "domain::**", "infra::**")
        }
    }

    /// `no-cycles` rule over `**` with the given `allow` entries.
    fn cycles_rule_allowing(entries: &[(&str, &str)]) -> Rule {
        Rule {
            allow: entries
                .iter()
                .map(|&(from, to)| allow_edge(from, to))
                .collect(),
            ..cycles_rule("no cycles", "**")
        }
    }

    fn config_of(rules: Vec<Rule>, diagnostics: Diagnostics) -> ArcConfig {
        ArcConfig { rules, diagnostics }
    }

    fn unlayered_node_except(names: &[&str]) -> Diagnostics {
        Diagnostics {
            unlayered_node: UnlayeredNode {
                level: DiagnosticLevel::Deny,
                except: names.iter().map(|&name| name.into()).collect(),
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
                DiagnosticKind::UnlayeredNode { entry } => Some(entry.node.as_str()),
                _ => None,
            })
            .collect()
    }

    fn unlayered_of(diagnostics: &[Diagnostic]) -> Vec<(&str, &str)> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::UnlayeredNode { entry } => {
                    Some((entry.rule.as_str(), entry.node.as_str()))
                }
                _ => None,
            })
            .collect()
    }

    fn unmatched_patterns(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::UnmatchedPattern { entry } => Some(entry.pattern.as_str()),
                _ => None,
            })
            .collect()
    }

    fn dead_catch_all_layers(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                DiagnosticKind::DeadCatchAllLayer { rule } => Some(rule.as_str()),
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

    // ===== unlayered-node =====

    #[test]
    fn a_rule_without_exhaustive_claims_nothing() {
        let graph = workspace(&["domain", "infra", "xtask"]);
        let config = config_of(
            vec![layers_rule("architecture layers", &["infra", "domain"])],
            Diagnostics::default(),
        );
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn an_exhaustive_crate_rule_claims_every_workspace_crate() {
        let graph = workspace(&["domain", "infra", "xtask"]);
        let config = config_of(
            vec![exhaustive_layers_rule(
                "architecture layers",
                &["infra", "domain"],
            )],
            Diagnostics::default(),
        );
        // `xtask::service` is not reported: no module pattern claims it.
        assert_eq!(unlayered(&diagnose(&graph, &config)), ["xtask"]);
    }

    #[test]
    fn an_exhaustive_module_rule_claims_the_modules_of_the_crates_it_touches() {
        let mut graph = ArcGraph::new();
        let app = add_crate(&mut graph, "app");
        add_module(&mut graph, "service", app, app);
        add_module(&mut graph, "store", app, app);
        let xtask = add_crate(&mut graph, "xtask");
        add_module(&mut graph, "service", xtask, xtask);

        let config = config_of(
            vec![exhaustive_layers_rule("app layers", &["app::service"])],
            Diagnostics::default(),
        );
        // Neither `app` nor `xtask` nor `xtask::service` is claimed: the
        // module pattern says nothing about the crate node or other crates.
        assert_eq!(unlayered(&diagnose(&graph, &config)), ["app::store"]);
    }

    #[test]
    fn an_exhaustive_module_rule_leaves_the_crate_node_alone() {
        // The assertion the repo's own arc-rules.toml rests on.
        let mut graph = ArcGraph::new();
        let app = add_crate(&mut graph, "app");
        add_module(&mut graph, "service", app, app);

        let config = config_of(
            vec![exhaustive_layers_rule("app layers", &["app::service"])],
            Diagnostics::default(),
        );
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn an_exhaustive_mixed_rule_makes_both_demands() {
        let mut graph = ArcGraph::new();
        let app = add_crate(&mut graph, "app");
        add_module(&mut graph, "service", app, app);
        add_module(&mut graph, "store", app, app);
        let infra = add_crate(&mut graph, "infra");
        add_module(&mut graph, "db", infra, infra);
        let xtask = add_crate(&mut graph, "xtask");
        add_module(&mut graph, "tool", xtask, xtask);

        let config = config_of(
            vec![exhaustive_layers_rule(
                "architecture layers",
                &["infra", "app::service"],
            )],
            Diagnostics::default(),
        );
        // The crate pattern `infra` claims all three crates, so `app` and
        // `xtask` are gaps; the module pattern claims `app`'s modules, so
        // `app::store` is a gap too, folded into `app` by the topmost filter.
        assert_eq!(unlayered(&diagnose(&graph, &config)), ["app", "xtask"]);
    }

    #[test]
    fn only_the_topmost_unsorted_module_is_reported() {
        let mut graph = ArcGraph::new();
        let app = add_crate(&mut graph, "app");
        let service = add_module(&mut graph, "service", app, app);
        add_module(&mut graph, "inner", app, service);
        add_module(&mut graph, "store", app, app);

        let config = config_of(
            vec![exhaustive_layers_rule("app layers", &["app::store"])],
            Diagnostics::default(),
        );
        assert_eq!(unlayered(&diagnose(&graph, &config)), ["app::service"]);
    }

    #[test]
    fn every_pattern_of_a_ranked_position_sorts() {
        // A position holding several patterns is the shape this repo's own
        // rules file uses, so all of them have to count, not just the first.
        let mut graph = ArcGraph::new();
        let app = add_crate(&mut graph, "app");
        add_module(&mut graph, "service", app, app);
        add_module(&mut graph, "store", app, app);
        add_module(&mut graph, "report", app, app);

        let rank: Layer = ["app::service", "app::store"]
            .into_iter()
            .map(String::from)
            .collect();
        let rule = Rule {
            kind: RuleKind::Layers(LayersRule {
                layers: vec![rank],
                direction: Direction::TopDown,
                exhaustive: true,
            }),
            ..layers_rule("app layers", &[])
        };
        let config = config_of(vec![rule], Diagnostics::default());
        assert_eq!(unlayered(&diagnose(&graph, &config)), ["app::report"]);
    }

    #[test]
    fn a_child_position_leaves_a_grandchild_reportable() {
        // `app::*` names the direct children only, so `inner` is unsorted with
        // a sorted parent: it is topmost itself and reported in its own right.
        let mut graph = ArcGraph::new();
        let app = add_crate(&mut graph, "app");
        let service = add_module(&mut graph, "service", app, app);
        add_module(&mut graph, "inner", app, service);
        add_module(&mut graph, "store", app, app);

        let config = config_of(
            vec![exhaustive_layers_rule("app layers", &["app::*"])],
            Diagnostics::default(),
        );
        assert_eq!(
            unlayered(&diagnose(&graph, &config)),
            ["app::service::inner"]
        );
    }

    #[test]
    fn an_exhaustive_module_pattern_that_matches_nothing_claims_nothing() {
        // A misspelled module pattern touches no crate, so the module claim has
        // nothing to range over and `unmatched-pattern` carries the report alone.
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![exhaustive_layers_rule("app layers", &["domain::srevice"])],
            Diagnostics::default(),
        );
        let found = diagnose(&graph, &config);
        assert!(unlayered(&found).is_empty());
        assert_eq!(unmatched_patterns(&found), ["domain::srevice"]);
    }

    #[test]
    fn an_exhaustive_crate_pattern_that_matches_nothing_still_claims_every_crate() {
        // The other half of the asymmetry: a crate pattern claims the workspace
        // whether or not it resolves, so the typo costs two diagnostics.
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![exhaustive_layers_rule(
                "architecture layers",
                &["dmoain", "infra"],
            )],
            Diagnostics::default(),
        );
        let found = diagnose(&graph, &config);
        assert_eq!(unlayered(&found), ["domain"]);
        assert_eq!(unmatched_patterns(&found), ["dmoain"]);
    }

    #[test]
    fn an_excepted_node_takes_its_modules_with_it() {
        let mut graph = ArcGraph::new();
        let app = add_crate(&mut graph, "app");
        let service = add_module(&mut graph, "service", app, app);
        add_module(&mut graph, "inner", app, service);
        add_module(&mut graph, "store", app, app);

        let config = config_of(
            vec![exhaustive_layers_rule("app layers", &["app::store"])],
            unlayered_node_except(&["app::service"]),
        );
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn each_exhaustive_rule_stands_for_itself() {
        let graph = workspace(&["domain", "infra", "tools"]);
        let config = config_of(
            vec![
                exhaustive_layers_rule("core layers", &["infra", "domain"]),
                exhaustive_layers_rule("tool layers", &["tools"]),
            ],
            Diagnostics::default(),
        );
        assert_eq!(
            unlayered_of(&diagnose(&graph, &config)),
            [
                ("core layers", "tools"),
                ("tool layers", "domain"),
                ("tool layers", "infra"),
            ]
        );
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
    fn an_exhaustive_rule_set_to_ignore_makes_no_claim() {
        let graph = workspace(&["domain", "xtask"]);
        let mut rule = exhaustive_layers_rule("architecture layers", &["domain"]);
        rule.severity = Severity::Ignore;
        let config = config_of(vec![rule], Diagnostics::default());
        assert!(unlayered(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn allow_silences_a_diagnostic() {
        let graph = workspace(&["domain", "xtask"]);
        let config = config_of(
            vec![exhaustive_layers_rule("architecture layers", &["domain"])],
            Diagnostics {
                unlayered_node: UnlayeredNode {
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
            vec![exhaustive_layers_rule("architecture layers", &["domain"])],
            Diagnostics {
                unlayered_node: UnlayeredNode {
                    level: DiagnosticLevel::Warn,
                    except: vec![],
                },
                ..Diagnostics::default()
            },
        );
        let found = diagnose(&graph, &config);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].level, DiagnosticLevel::Warn);
    }

    // ===== unmatched-allow =====

    #[test]
    fn allow_pattern_matching_no_module_is_a_dead_entry() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule(
                "no infra in domain",
                vec![allow_edge("domain::typo", "infra::service")],
            )],
            Diagnostics::default(),
        );
        let dead = dead_allows(&PatternIndex::build(&graph), &config);
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].rule, "no infra in domain");
        assert_eq!(dead[0].pattern, "domain::typo");
    }

    #[test]
    fn a_resolving_allow_is_not_dead() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule(
                "no infra in domain",
                vec![allow_edge("domain::service", "infra::service")],
            )],
            Diagnostics::default(),
        );
        assert!(dead_allows(&PatternIndex::build(&graph), &config).is_empty());
    }

    #[test]
    fn a_relative_entry_is_asked_about_its_from_side_only() {
        let graph = workspace(&["domain"]);
        let config = config_of(
            vec![cycles_rule_allowing(&[
                ("domain::service", "super"),
                ("domain::typo", "crate"),
            ])],
            Diagnostics::default(),
        );
        let dead = dead_allows(&PatternIndex::build(&graph), &config);
        let patterns: Vec<&str> = dead.iter().map(|d| d.pattern.as_str()).collect();
        assert_eq!(patterns, ["domain::typo"]);
    }

    #[test]
    fn dead_allow_is_reported_as_a_diagnostic() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_rule(
                "no infra in domain",
                vec![allow_edge("domain::typo", "infra::service")],
            )],
            Diagnostics::default(),
        );
        let found = diagnose(&graph, &config);
        assert!(
            found.iter().any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::UnmatchedAllow { entry }
                    if entry.rule == "no infra in domain" && entry.pattern == "domain::typo"
            )),
            "got: {found:?}"
        );
    }

    // ===== contradictory-allow =====

    /// Crate `core` holding `a`, which holds `deep`, beside `b` and `error`.
    fn nested_graph() -> ArcGraph {
        let mut graph = ArcGraph::new();
        let core = add_crate(&mut graph, "core");
        let a = add_module(&mut graph, "a", core, core);
        add_module(&mut graph, "deep", core, a);
        add_module(&mut graph, "b", core, core);
        add_module(&mut graph, "error", core, core);
        graph
    }

    fn contradictions(graph: &ArcGraph, entries: &[(&str, &str)]) -> Vec<ContradictoryAllow> {
        let config = config_of(vec![cycles_rule_allowing(entries)], Diagnostics::default());
        diagnose(graph, &config)
            .into_iter()
            .filter_map(|diagnostic| match diagnostic.kind {
                DiagnosticKind::ContradictoryAllow { entry } => {
                    assert_eq!(diagnostic.level, DiagnosticLevel::Deny);
                    Some(entry)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn two_entries_putting_a_pair_above_each_other_contradict() {
        let found = contradictions(
            &nested_graph(),
            &[("core::a", "core::b"), ("core::b", "core::a")],
        );
        assert_eq!(found.len(), 1, "got: {found:?}");
        assert_eq!(found[0].rule, "no cycles");
        assert_eq!(
            found[0].entries,
            ["core::a -> core::b", "core::b -> core::a"]
        );
        assert_eq!(found[0].cycle, ["core::a", "core::b", "core::a"]);
    }

    #[test]
    fn the_tree_reading_and_its_mirror_contradict_on_every_pair() {
        let found = contradictions(&nested_graph(), &[("**", "super"), ("**", "self::*")]);
        assert_eq!(found.len(), 1, "got: {found:?}");
        assert_eq!(found[0].entries, ["** -> super", "** -> self::*"]);
        assert_eq!(found[0].cycle.len(), 3, "got: {:?}", found[0].cycle);
    }

    #[test]
    fn one_direction_declares_an_order() {
        assert!(contradictions(&nested_graph(), &[("**", "super")]).is_empty());
        assert!(contradictions(&nested_graph(), &[("**", "crate")]).is_empty());
    }

    #[test]
    fn an_entry_with_the_same_pattern_on_both_sides_ranks_nothing() {
        assert!(contradictions(&nested_graph(), &[("core", "core")]).is_empty());
    }

    /// `core` as a pattern takes its subtree with it, so the entry covers the
    /// sibling pair `a`/`b` in both directions. That leaves the pair unranked
    /// rather than ranked both ways, and `a -> core` still ranks.
    #[test]
    fn an_entry_covering_a_pair_both_ways_ranks_that_pair_neither_way() {
        assert!(contradictions(&nested_graph(), &[("core::**", "core")]).is_empty());
        let found = contradictions(
            &nested_graph(),
            &[("core::**", "core"), ("core", "core::a")],
        );
        assert_eq!(found.len(), 1, "got: {found:?}");
        assert_eq!(found[0].cycle, ["core", "core::a", "core"]);
    }

    #[test]
    fn an_entry_whose_sides_overlap_is_no_contradiction_with_itself() {
        assert!(contradictions(&nested_graph(), &[("core::error", "core::*")]).is_empty());
    }

    #[test]
    fn a_contradiction_at_allow_is_not_collected() {
        let config = config_of(
            vec![cycles_rule_allowing(&[
                ("core::a", "core::b"),
                ("core::b", "core::a"),
            ])],
            Diagnostics {
                contradictory_allow: DiagnosticLevel::Allow,
                ..Diagnostics::default()
            },
        );
        assert!(
            diagnose(&nested_graph(), &config)
                .iter()
                .all(|d| !matches!(d.kind, DiagnosticKind::ContradictoryAllow { .. }))
        );
    }

    // ===== unmatched-pattern =====

    #[test]
    fn rule_pattern_matching_no_module_is_reported() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_between(
                "no infra in domain",
                "domian::**",
                "infra::**",
            )],
            Diagnostics::default(),
        );
        assert_eq!(
            unmatched_patterns(&diagnose(&graph, &config)),
            ["domian::**"]
        );
    }

    /// A rules file covers the workspace, so `crate::` names nothing: the
    /// pattern misses like any typo, and this diagnostic is what says so.
    #[test]
    fn a_crate_prefixed_pattern_misses_like_any_other() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_between(
                "no infra in domain",
                "crate::domain",
                "infra::**",
            )],
            Diagnostics::default(),
        );
        assert_eq!(
            unmatched_patterns(&diagnose(&graph, &config)),
            ["crate::domain"]
        );
    }

    #[test]
    fn a_scope_matching_no_module_is_reported() {
        let graph = workspace(&["domain"]);
        let config = config_of(
            vec![cycles_rule("domain acyclic", "domian::**")],
            Diagnostics::default(),
        );
        assert_eq!(
            unmatched_patterns(&diagnose(&graph, &config)),
            ["domian::**"]
        );
    }

    #[test]
    fn a_layer_entry_matching_no_module_is_reported() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![layers_rule("architecture layers", &["infra", "domian"])],
            Diagnostics::default(),
        );
        assert_eq!(unmatched_patterns(&diagnose(&graph, &config)), ["domian"]);
    }

    #[test]
    fn a_rule_set_to_ignore_has_no_dead_pattern() {
        // Same reading as everywhere else: an unchecked rule states nothing,
        // so its patterns have no reach to fall short of.
        let graph = workspace(&["domain", "infra"]);
        let mut rule = forbidden_between("no infra in domain", "domian::**", "infra::**");
        rule.severity = Severity::Ignore;
        let config = config_of(vec![rule], Diagnostics::default());
        assert!(unmatched_patterns(&diagnose(&graph, &config)).is_empty());
    }

    #[test]
    fn an_unmatched_pattern_is_denied_by_default() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_between(
                "no infra in domain",
                "domian::**",
                "infra::**",
            )],
            Diagnostics::default(),
        );
        let found = diagnose(&graph, &config);
        let levels: Vec<DiagnosticLevel> = found
            .iter()
            .filter(|diagnostic| diagnostic.name() == "unmatched-pattern")
            .map(|diagnostic| diagnostic.level)
            .collect();
        assert_eq!(levels, [DiagnosticLevel::Deny]);
    }

    /// The ordinary positions already name every crate in the fixture, so the
    /// catch-all catches nothing: the same class of dead configuration as a
    /// pattern that matches no module, reported under the same diagnostic name.
    #[test]
    fn a_catch_all_whose_rest_is_empty_is_reported_as_dead() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![layers_rule(
                "architecture layers",
                &["domain", "infra", "*"],
            )],
            Diagnostics::default(),
        );
        assert_eq!(
            dead_catch_all_layers(&diagnose(&graph, &config)),
            ["architecture layers"]
        );
    }

    #[test]
    fn allow_silences_an_unmatched_pattern() {
        let graph = workspace(&["domain", "infra"]);
        let config = config_of(
            vec![forbidden_between(
                "no infra in domain",
                "domian::**",
                "infra::**",
            )],
            Diagnostics {
                unmatched_pattern: DiagnosticLevel::Allow,
                ..Diagnostics::default()
            },
        );
        assert!(unmatched_patterns(&diagnose(&graph, &config)).is_empty());
    }

    // ===== unmatched-baseline-entry =====

    fn stale_entry(rule: &str) -> BaselineEntry {
        BaselineEntry {
            rule: rule.into(),
            key: ViolationKey {
                edge: Edge::new("domain::service", "infra::service"),
                symbols: EdgeSymbols {
                    named: ["Service".to_string()].into_iter().collect(),
                    bare: false,
                },
            },
        }
    }

    #[test]
    fn baseline_entry_no_violation_matched_is_reported() {
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
        // frozen violations as stale would turn that statement around.
        let graph = workspace(&["domain", "infra"]);
        let mut rule = forbidden_rule("no infra in domain", vec![]);
        rule.severity = Severity::Ignore;
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
