//! Module path pattern resolution
//!
//! Resolves module path patterns like `domain::*` or `domain::**` to concrete
//! `NodeIndex` sets in the `ArcGraph`.

use crate::graph::{ArcGraph, EdgeWeight};
use crate::rules::config::{AllowTarget, AllowedEdge, Layer};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;

fn non_external_indices(graph: &ArcGraph) -> impl Iterator<Item = NodeIndex> + '_ {
    graph
        .node_indices()
        .filter(|&idx| !graph[idx].is_external())
}

/// Every non-external node of one graph, by qualified name. Bound to that
/// graph, so an index cannot be used against a different one.
pub struct PatternIndex<'graph> {
    graph: &'graph ArcGraph,
    names: std::collections::HashMap<String, NodeIndex>,
}

impl<'graph> PatternIndex<'graph> {
    #[must_use]
    pub(super) fn build(graph: &'graph ArcGraph) -> Self {
        Self {
            graph,
            names: non_external_indices(graph)
                .map(|idx| (graph.qualified_name(idx), idx))
                .collect(),
        }
    }

    pub(super) fn graph(&self) -> &'graph ArcGraph {
        self.graph
    }

    fn get(&self, pattern: &str) -> Option<NodeIndex> {
        self.names.get(pattern).copied()
    }

    /// Resolve a module path pattern to matching graph nodes.
    ///
    /// Supported patterns:
    /// - `"domain"` — the crate and every module in it
    /// - `"domain::service"` — that module and every module in it
    /// - `"domain::*"` — the direct children of `domain`
    /// - `"domain::**"` — every module in `domain`, not `domain` itself
    /// - `*` inside a name segment matches any run of characters, never `::`;
    ///   a name it matches contributes its whole containment subtree, same as
    ///   an exact name would.
    #[must_use]
    pub(super) fn resolve(&self, pattern: &str) -> Vec<NodeIndex> {
        let graph = self.graph;

        // Bare `**` matches all non-external nodes
        if pattern == "**" {
            return non_external_indices(graph).collect();
        }

        // Check for wildcard suffix
        if let Some(base) = pattern.strip_suffix("::**") {
            return self.resolve_descendants(base);
        }
        if let Some(base) = pattern.strip_suffix("::*") {
            return self.resolve_children(base);
        }

        let mut result = HashSet::new();
        for idx in self.matching_indices(pattern) {
            result.extend(graph.containment_subtree(idx));
        }
        result.into_iter().collect()
    }

    /// `domain::*` — direct children of every node `base_pattern` names.
    fn resolve_children(&self, base_pattern: &str) -> Vec<NodeIndex> {
        self.matching_indices(base_pattern)
            .into_iter()
            .flat_map(|base_idx| {
                self.graph
                    .edges(base_idx)
                    .filter(|edge| matches!(edge.weight(), EdgeWeight::Contains))
                    .map(|edge| edge.target())
                    .collect::<Vec<_>>()
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }

    /// `domain::**` — transitive descendants of every node `base_pattern`
    /// names, excluding those nodes themselves.
    fn resolve_descendants(&self, base_pattern: &str) -> Vec<NodeIndex> {
        let mut result = HashSet::new();
        for base_idx in self.matching_indices(base_pattern) {
            let mut subtree = self.graph.containment_subtree(base_idx);
            subtree.remove(&base_idx);
            result.extend(subtree);
        }
        result.into_iter().collect()
    }

    /// Every non-external node that `layers`'s ordinary positions do not
    /// match: the catch-all's own reach. `None` when `layers` carries no
    /// catch-all, the one situation this set has a use. Shared by the
    /// `layers` check, which assigns this set to the catch-all's position,
    /// and the `unmatched-pattern` diagnostic, which reports the catch-all as
    /// dead when this set is empty.
    pub(super) fn layer_rest(&self, layers: &[Layer]) -> Option<HashSet<NodeIndex>> {
        if !layers.iter().any(Layer::is_catch_all) {
            return None;
        }
        let matched: HashSet<NodeIndex> = layers
            .iter()
            .filter_map(Layer::patterns)
            .flatten()
            .flat_map(|pattern| self.resolve(pattern))
            .collect();
        Some(
            non_external_indices(self.graph)
                .filter(|idx| !matched.contains(idx))
                .collect(),
        )
    }

    /// Nodes named by `pattern`, without expanding into their subtrees.
    ///
    /// A wildcard-free pattern is a single `HashMap` lookup, so rule files
    /// without `*` see no slowdown. A pattern containing `*` scans
    /// `self.names`, matching qualified names segment by segment.
    fn matching_indices(&self, pattern: &str) -> Vec<NodeIndex> {
        if !pattern.contains('*') {
            return self.get(pattern).into_iter().collect();
        }
        let pattern_segments: Vec<&str> = pattern.split("::").collect();
        self.names
            .iter()
            .filter(|(name, _)| {
                let name_segments: Vec<&str> = name.split("::").collect();
                name_segments.len() == pattern_segments.len()
                    && pattern_segments
                        .iter()
                        .zip(&name_segments)
                        .all(|(&p, &n)| segment_matches(p, n))
            })
            .map(|(_, &idx)| idx)
            .collect()
    }
}

/// The `allow` entries of one rule, resolved once against the graph so a
/// per-edge check is a set lookup or a walk along the containment chain
/// rather than a re-run of pattern resolution.
pub(super) struct ResolvedAllows<'entries, 'graph> {
    graph: &'graph ArcGraph,
    entries: Vec<ResolvedAllow<'entries>>,
    parent_of: HashMap<NodeIndex, NodeIndex>,
    children_of: HashMap<NodeIndex, Vec<NodeIndex>>,
}

struct ResolvedAllow<'entries> {
    entry: &'entries AllowedEdge,
    from: HashSet<NodeIndex>,
    to: ResolvedTarget,
}

/// An absolute `to` is one node set for every `from` node; a relative one is
/// read off the containment tree per `from` node at check time.
enum ResolvedTarget {
    Nodes(HashSet<NodeIndex>),
    Ancestor(NonZeroUsize),
    Crate,
    Children,
    Descendants,
}

impl<'entries, 'graph> ResolvedAllows<'entries, 'graph> {
    pub(super) fn resolve(entries: &'entries [AllowedEdge], index: &PatternIndex<'graph>) -> Self {
        let graph = index.graph();
        let parent_of = graph.parent_map();
        let mut children_of: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        for (&child, &parent) in &parent_of {
            children_of.entry(parent).or_default().push(child);
        }
        Self {
            graph,
            entries: entries
                .iter()
                .map(|entry| ResolvedAllow {
                    entry,
                    from: index.resolve(&entry.from).into_iter().collect(),
                    to: match &entry.to {
                        AllowTarget::Path(pattern) => {
                            ResolvedTarget::Nodes(index.resolve(pattern).into_iter().collect())
                        }
                        AllowTarget::Ancestor(levels) => ResolvedTarget::Ancestor(*levels),
                        AllowTarget::Crate => ResolvedTarget::Crate,
                        AllowTarget::Children => ResolvedTarget::Children,
                        AllowTarget::Descendants => ResolvedTarget::Descendants,
                    },
                })
                .collect(),
            parent_of,
            children_of,
        }
    }

    /// Whether the rule carries no entry at all, so per-edge work can be
    /// skipped entirely.
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The first entry whose `from` matches `source` and whose `to` names
    /// `target` from there.
    pub(super) fn covers(
        &self,
        source: NodeIndex,
        target: NodeIndex,
    ) -> Option<&'entries AllowedEdge> {
        self.entries
            .iter()
            .find(|resolved| {
                resolved.from.contains(&source) && self.reaches(&resolved.to, source, target)
            })
            .map(|resolved| resolved.entry)
    }

    /// Every (source, target) pair some entry covers, with the position of
    /// that entry in the list handed to `resolve`. Pairs of one node with
    /// itself are left out: a node is not above itself, and no edge runs
    /// there.
    pub(super) fn pairs(&self) -> impl Iterator<Item = (NodeIndex, NodeIndex, usize)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .flat_map(move |(position, resolved)| {
                resolved.from.iter().flat_map(move |&source| {
                    self.targets_from(&resolved.to, source)
                        .into_iter()
                        .filter(move |&target| target != source)
                        .map(move |target| (source, target, position))
                })
            })
    }

    fn reaches(&self, to: &ResolvedTarget, source: NodeIndex, target: NodeIndex) -> bool {
        match to {
            ResolvedTarget::Nodes(nodes) => nodes.contains(&target),
            ResolvedTarget::Ancestor(levels) => self.ancestor_of(source, *levels) == Some(target),
            ResolvedTarget::Crate => self.crate_of(source) == Some(target),
            ResolvedTarget::Children => self.parent_of.get(&target) == Some(&source),
            ResolvedTarget::Descendants => {
                let mut node = target;
                while let Some(&parent) = self.parent_of.get(&node) {
                    if parent == source {
                        return true;
                    }
                    node = parent;
                }
                false
            }
        }
    }

    fn targets_from(&self, to: &ResolvedTarget, source: NodeIndex) -> Vec<NodeIndex> {
        match to {
            ResolvedTarget::Nodes(nodes) => nodes.iter().copied().collect(),
            ResolvedTarget::Ancestor(levels) => {
                self.ancestor_of(source, *levels).into_iter().collect()
            }
            ResolvedTarget::Crate => self.crate_of(source).into_iter().collect(),
            ResolvedTarget::Children => self.children_of.get(&source).cloned().unwrap_or_default(),
            ResolvedTarget::Descendants => {
                let mut found = Vec::new();
                let mut stack = self.children_of.get(&source).cloned().unwrap_or_default();
                while let Some(node) = stack.pop() {
                    stack.extend(self.children_of.get(&node).into_iter().flatten());
                    found.push(node);
                }
                found
            }
        }
    }

    /// The node `levels` steps up the containment chain from `source`, or
    /// `None` where the chain ends first.
    fn ancestor_of(&self, source: NodeIndex, levels: NonZeroUsize) -> Option<NodeIndex> {
        let mut node = source;
        for _ in 0..levels.get() {
            node = *self.parent_of.get(&node)?;
        }
        Some(node)
    }

    /// The crate root of a module; `None` for a crate node, which `crate`
    /// does not point back at.
    fn crate_of(&self, source: NodeIndex) -> Option<NodeIndex> {
        self.graph[source]
            .is_module()
            .then(|| self.graph.owning_crate(source))
    }
}

/// Whether `*` in `pattern` can stretch to make `name` match, where `*`
/// stands for any run of characters, including none, within this one
/// segment. Both are already free of `::`.
fn segment_matches(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut remainder) = name.strip_prefix(first) else {
        return false;
    };

    let mut parts = parts.peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return remainder.ends_with(part);
        }
        if part.is_empty() {
            continue;
        }
        let Some(pos) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[pos + part.len()..];
    }

    remainder.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::test_support::allow_edge;
    use crate::test_support::{crate_node, module_node};

    /// Build a test graph with a crate "test" and modules beneath it.
    /// Returns (graph, `crate_idx`).
    fn test_crate_graph() -> (ArcGraph, NodeIndex) {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(crate_node("test"));
        (graph, crate_idx)
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

    #[test]
    fn test_resolve_exact_module() {
        let (mut graph, crate_idx) = test_crate_graph();
        let service = add_module(&mut graph, "service", crate_idx, crate_idx);
        let inner = add_module(&mut graph, "inner", crate_idx, service);
        let leaf = add_module(&mut graph, "leaf", crate_idx, inner);
        // Sibling of the pattern, so the result must stop at the subtree.
        let _other = add_module(&mut graph, "other", crate_idx, crate_idx);
        let mut result = PatternIndex::build(&graph).resolve("test::service");
        result.sort_unstable();
        let mut expected = vec![service, inner, leaf];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_crate() {
        let (mut graph, crate_idx) = test_crate_graph();
        let mod_a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let mod_b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let mut result = PatternIndex::build(&graph).resolve("test");
        result.sort_unstable();
        let mut expected = vec![crate_idx, mod_a, mod_b];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_children() {
        let (mut graph, crate_idx) = test_crate_graph();
        let mod_a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let mod_b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // Grandchild should NOT be included with *
        let _grandchild = add_module(&mut graph, "deep", crate_idx, mod_a);
        let mut result = PatternIndex::build(&graph).resolve("test::*");
        result.sort_unstable();
        let mut expected = vec![mod_a, mod_b];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_descendants() {
        let (mut graph, crate_idx) = test_crate_graph();
        let mod_a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let mod_b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let grandchild = add_module(&mut graph, "deep", crate_idx, mod_a);
        let mut result = PatternIndex::build(&graph).resolve("test::**");
        result.sort_unstable();
        let mut expected = vec![mod_a, mod_b, grandchild];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_nonexistent() {
        let (graph, _) = test_crate_graph();
        let result = PatternIndex::build(&graph).resolve("nonexistent");
        assert!(result.is_empty());
    }

    /// A relative `to` that leads off the containment tree matches nothing:
    /// no node stands two levels above a top-level module, the crate node
    /// has no crate above it, and a leaf has no children.
    #[test]
    fn test_a_relative_target_leading_nowhere_covers_nothing() {
        let (mut graph, crate_idx) = test_crate_graph();
        let top = add_module(&mut graph, "top", crate_idx, crate_idx);
        let leaf = add_module(&mut graph, "leaf", crate_idx, top);
        let index = PatternIndex::build(&graph);
        let entry = |to: &str| allow_edge("**", to);

        let covered = |to: &str, source: NodeIndex, target: NodeIndex| {
            let entries = [entry(to)];
            ResolvedAllows::resolve(&entries, &index)
                .covers(source, target)
                .is_some()
        };
        assert!(covered("super::super", leaf, crate_idx));
        assert!(!covered("super::super", top, crate_idx));
        assert!(covered("crate", top, crate_idx));
        assert!(!covered("crate", crate_idx, crate_idx));
        assert!(covered("self::*", top, leaf));
        assert!(!covered("self::*", leaf, top));

        let entries = [entry("super::super"), entry("crate"), entry("self::*")];
        let pairs: Vec<_> = ResolvedAllows::resolve(&entries, &index).pairs().collect();
        assert_eq!(
            pairs.len(),
            5,
            "leaf: super::super, crate; top: crate, self::*; crate: self::*; got {pairs:?}"
        );
    }

    fn add_crate(graph: &mut ArcGraph, name: &str) -> NodeIndex {
        graph.add_node(crate_node(name))
    }

    #[test]
    fn test_resolve_wildcard_crate_suffix_brings_modules() {
        let mut graph = ArcGraph::new();
        let orders = add_crate(&mut graph, "app_orders");
        let orders_service = add_module(&mut graph, "service", orders, orders);
        let orders_v2 = add_crate(&mut graph, "app_orders_v2");
        let orders_v2_service = add_module(&mut graph, "service", orders_v2, orders_v2);
        let billing = add_crate(&mut graph, "app_billing");

        let mut result = PatternIndex::build(&graph).resolve("app_orders*");
        result.sort_unstable();
        let mut expected = vec![orders, orders_service, orders_v2, orders_v2_service];
        expected.sort_unstable();
        assert_eq!(result, expected);
        assert!(!result.contains(&billing));
    }

    #[test]
    fn test_resolve_wildcard_infix_matches_middle_of_name() {
        let mut graph = ArcGraph::new();
        let orders = add_crate(&mut graph, "app_orders");
        let billing = add_crate(&mut graph, "app_billing");

        let result = PatternIndex::build(&graph).resolve("*orders*");
        assert_eq!(result, vec![orders]);
        assert!(!result.contains(&billing));
    }

    #[test]
    fn test_resolve_wildcard_does_not_cross_segment_boundary() {
        let mut graph = ArcGraph::new();
        let core = add_crate(&mut graph, "core");
        let service = add_module(&mut graph, "service", core, core);
        let _inner = add_module(&mut graph, "inner", core, service);

        let result = PatternIndex::build(&graph).resolve("*::inner");
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_wildcard_base_before_descendants() {
        let mut graph = ArcGraph::new();
        let orders = add_crate(&mut graph, "app_orders");
        let orders_service = add_module(&mut graph, "service", orders, orders);
        let billing = add_crate(&mut graph, "app_billing");
        let billing_service = add_module(&mut graph, "service", billing, billing);
        let infra = add_crate(&mut graph, "infra_db");

        let mut result = PatternIndex::build(&graph).resolve("app_*::**");
        result.sort_unstable();
        let mut expected = vec![orders_service, billing_service];
        expected.sort_unstable();
        assert_eq!(result, expected);
        assert!(!result.contains(&orders));
        assert!(!result.contains(&billing));
        assert!(!result.contains(&infra));
    }

    #[test]
    fn test_resolve_wildcard_no_match_is_empty() {
        let (graph, _) = test_crate_graph();
        let result = PatternIndex::build(&graph).resolve("no_such_*");
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_bare_double_star_matches_all_non_external() {
        let mut graph = ArcGraph::new();
        let orders = add_crate(&mut graph, "app_orders");
        let orders_service = add_module(&mut graph, "service", orders, orders);
        let billing = add_crate(&mut graph, "app_billing");

        let mut result = PatternIndex::build(&graph).resolve("**");
        result.sort_unstable();
        let mut expected = vec![orders, orders_service, billing];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }
}
