//! Module path pattern resolution
//!
//! Resolves module path patterns like `domain::*` or `domain::**` to concrete
//! `NodeIndex` sets in the `ArcGraph`.

use crate::graph::{ArcGraph, Edge};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

/// Resolve a module path pattern to matching graph nodes.
///
/// Supported patterns:
/// - `"domain"` — crate node + all contained modules
/// - `"domain::service"` — exact module match
/// - `"domain::*"` — direct children of `domain`
/// - `"domain::**"` — all transitive descendants of `domain`
/// - `"crate::domain"` — `crate::` prefix is stripped
#[must_use]
pub fn resolve_pattern(pattern: &str, graph: &ArcGraph) -> Vec<NodeIndex> {
    let pattern = pattern.strip_prefix("crate::").unwrap_or(pattern);

    // Bare `**` matches all non-external nodes
    if pattern == "**" {
        return graph
            .node_indices()
            .filter(|&idx| !graph[idx].is_external())
            .collect();
    }

    // Check for wildcard suffix
    if let Some(base) = pattern.strip_suffix("::**") {
        return resolve_glob(base, graph);
    }
    if let Some(base) = pattern.strip_suffix("::*") {
        return resolve_wildcard(base, graph);
    }

    // Build path index: qualified name → NodeIndex
    let path_index = build_path_index(graph);

    // Exact match first (could be a module path like "domain::service")
    if let Some(&idx) = path_index.get(pattern) {
        // If it's a crate node, return crate + all descendants
        if graph[idx].is_crate() {
            return graph.containment_subtree(idx).into_iter().collect();
        }
        return vec![idx];
    }

    Vec::new()
}

fn build_path_index(graph: &ArcGraph) -> std::collections::HashMap<String, NodeIndex> {
    graph
        .node_indices()
        .filter(|&idx| !graph[idx].is_external())
        .map(|idx| (graph.qualified_name(idx), idx))
        .collect()
}

/// `domain::*` — direct children only (via Contains edges).
fn resolve_wildcard(base_pattern: &str, graph: &ArcGraph) -> Vec<NodeIndex> {
    let path_index = build_path_index(graph);
    let Some(&base_idx) = path_index.get(base_pattern) else {
        return Vec::new();
    };
    graph
        .edges(base_idx)
        .filter(|edge| matches!(edge.weight(), Edge::Contains))
        .map(|edge| edge.target())
        .collect()
}

/// `domain::**` — all transitive descendants (excluding the root itself).
fn resolve_glob(base_pattern: &str, graph: &ArcGraph) -> Vec<NodeIndex> {
    let path_index = build_path_index(graph);
    let Some(&base_idx) = path_index.get(base_pattern) else {
        return Vec::new();
    };
    let mut subtree = graph.containment_subtree(base_idx);
    subtree.remove(&base_idx);
    subtree.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;
    use std::path::PathBuf;

    /// Build a test graph with a crate "test" and modules beneath it.
    /// Returns (graph, crate_idx).
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

    #[test]
    fn test_resolve_exact_module() {
        let (mut graph, crate_idx) = test_crate_graph();
        let service = add_module(&mut graph, "service", crate_idx, crate_idx);
        let result = resolve_pattern("test::service", &graph);
        assert_eq!(result, vec![service]);
    }

    #[test]
    fn test_resolve_crate() {
        let (mut graph, crate_idx) = test_crate_graph();
        let mod_a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let mod_b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let mut result = resolve_pattern("test", &graph);
        result.sort_unstable();
        let mut expected = vec![crate_idx, mod_a, mod_b];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_wildcard() {
        let (mut graph, crate_idx) = test_crate_graph();
        let mod_a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let mod_b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // Grandchild should NOT be included with *
        let _grandchild = add_module(&mut graph, "deep", crate_idx, mod_a);
        let mut result = resolve_pattern("test::*", &graph);
        result.sort_unstable();
        let mut expected = vec![mod_a, mod_b];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_glob() {
        let (mut graph, crate_idx) = test_crate_graph();
        let mod_a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let mod_b = add_module(&mut graph, "b", crate_idx, crate_idx);
        let grandchild = add_module(&mut graph, "deep", crate_idx, mod_a);
        let mut result = resolve_pattern("test::**", &graph);
        result.sort_unstable();
        let mut expected = vec![mod_a, mod_b, grandchild];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_nonexistent() {
        let (graph, _) = test_crate_graph();
        let result = resolve_pattern("nonexistent", &graph);
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_crate_prefix_stripped() {
        let (mut graph, crate_idx) = test_crate_graph();
        let mod_a = add_module(&mut graph, "a", crate_idx, crate_idx);
        let mod_b = add_module(&mut graph, "b", crate_idx, crate_idx);
        // "crate::test" should resolve the same as "test"
        let mut result = resolve_pattern("crate::test", &graph);
        result.sort_unstable();
        let mut expected = vec![crate_idx, mod_a, mod_b];
        expected.sort_unstable();
        assert_eq!(result, expected);
    }
}
