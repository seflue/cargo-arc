//! Module path pattern resolution
//!
//! Resolves module path patterns like `domain::*` or `domain::**` to concrete
//! `NodeIndex` sets in the `ArcGraph`.

use crate::graph::{ArcGraph, EdgeWeight};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

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

        if let Some(idx) = self.get(pattern) {
            return graph.containment_subtree(idx).into_iter().collect();
        }

        Vec::new()
    }

    /// `domain::*` — direct children only (via Contains edges).
    fn resolve_children(&self, base_pattern: &str) -> Vec<NodeIndex> {
        let Some(base_idx) = self.get(base_pattern) else {
            return Vec::new();
        };
        self.graph
            .edges(base_idx)
            .filter(|edge| matches!(edge.weight(), EdgeWeight::Contains))
            .map(|edge| edge.target())
            .collect()
    }

    /// `domain::**` — all transitive descendants (excluding the root itself).
    fn resolve_descendants(&self, base_pattern: &str) -> Vec<NodeIndex> {
        let Some(base_idx) = self.get(base_pattern) else {
            return Vec::new();
        };
        let mut subtree = self.graph.containment_subtree(base_idx);
        subtree.remove(&base_idx);
        subtree.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;
    use std::path::PathBuf;

    /// Build a test graph with a crate "test" and modules beneath it.
    /// Returns (graph, `crate_idx`).
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
}
