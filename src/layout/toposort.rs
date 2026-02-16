//! Topological sorting algorithms for layout ordering.

use itertools::Itertools;
use petgraph::Direction;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Alias for local subgraph node indices (to distinguish from original ArcGraph `NodeIndex`).
type LocalIdx = petgraph::graph::NodeIndex;

/// Stable topological sort using Kahn's algorithm.
/// Preserves alphabetical order for nodes without dependency relationships.
pub(super) fn stable_toposort(
    sibling_deps: &DiGraph<NodeIndex, usize>,
    get_name: impl Fn(NodeIndex) -> String,
) -> Vec<NodeIndex> {
    if sibling_deps.node_count() == 0 {
        return vec![];
    }

    let ctx = ToposortGraph {
        node_to_orig: sibling_deps
            .node_indices()
            .map(|node| (node, sibling_deps[node]))
            .collect(),
        subgraph: sibling_deps,
        get_name,
    };

    // NOTE: This duplicates the Kahn's algorithm structure from `kahns_toposort`,
    // but cannot reuse it because the barycenter priority depends on `positions`
    // which is mutated during iteration (each placed node affects subsequent priorities).

    // Compute in-degrees
    let mut in_degree = vec![0usize; sibling_deps.node_count()];
    for node in sibling_deps.node_indices() {
        in_degree[node.index()] = sibling_deps
            .edges_directed(node, Direction::Incoming)
            .count();
    }

    // Barycenter score: average position of already-placed dependents.
    // Nodes closer to their dependents get lower scores → fewer arc crossings.
    let barycenter = |node: LocalIdx, positions: &HashMap<LocalIdx, usize>| -> f64 {
        let (sum, count) = sibling_deps
            .neighbors_directed(node, Direction::Incoming)
            .filter_map(|predecessor| positions.get(&predecessor).copied())
            .fold((0usize, 0usize), |(sum, count), pos| (sum + pos, count + 1));
        if count == 0 {
            f64::INFINITY
        } else {
            sum as f64 / count as f64
        }
    };

    // Position tracking: maps local node index → placement order in result
    let mut positions: HashMap<LocalIdx, usize> = HashMap::new();

    // Initialize with nodes having in-degree 0 (all INFINITY — nothing placed yet)
    let mut heap: BinaryHeap<Reverse<(Score, String, LocalIdx)>> = sibling_deps
        .node_indices()
        .filter(|&node| in_degree[node.index()] == 0)
        .map(|node| Reverse((Score(f64::INFINITY), ctx.name(node), node)))
        .collect();

    let mut result = Vec::new();
    while let Some(Reverse((_, _, node))) = heap.pop() {
        positions.insert(node, result.len());
        result.push(ctx.orig(node));

        // Decrease in-degree of neighbors
        for neighbor in sibling_deps.neighbors(node) {
            let degree = &mut in_degree[neighbor.index()];
            *degree -= 1;
            if *degree == 0 {
                let score = barycenter(neighbor, &positions);
                heap.push(Reverse((Score(score), ctx.name(neighbor), neighbor)));
            }
        }
    }

    // If not all nodes processed, there's a cycle — use SCC fallback
    if result.len() != sibling_deps.node_count() {
        return ctx.scc_fallback_sort();
    }

    result
}

/// Read-only context for toposort operations on a subgraph.
struct ToposortGraph<'a, F> {
    subgraph: &'a DiGraph<NodeIndex, usize>,
    node_to_orig: HashMap<LocalIdx, NodeIndex>,
    get_name: F,
}

impl<F: Fn(NodeIndex) -> String> ToposortGraph<'_, F> {
    fn orig(&self, local: LocalIdx) -> NodeIndex {
        self.node_to_orig[&local]
    }

    fn name(&self, local: LocalIdx) -> String {
        (self.get_name)(self.orig(local))
    }

    /// SCC condensation fallback when cycles exist.
    /// Applies ADR-017 pattern: condense → toposort → expand alphabetically.
    fn scc_fallback_sort(&self) -> Vec<NodeIndex> {
        let sccs = tarjan_scc(self.subgraph);
        let (condensed, _node_to_scc) = condense_sccs(self.subgraph, &sccs);

        // Stable Kahn's toposort on condensed DAG with alphabetical tiebreaker
        let scc_sort_name = |scc_node: LocalIdx| -> String {
            condensed[scc_node]
                .iter()
                .map(|&node| self.name(node))
                .min()
                .unwrap_or_default()
        };
        let scc_order = kahns_toposort(&condensed, scc_sort_name);

        // Log cycles (SCCs with >1 member)
        for &scc_node in &scc_order {
            let members = &condensed[scc_node];
            if members.len() > 1 {
                let names: Vec<String> = members.iter().map(|&node| self.name(node)).collect();
                tracing::debug!("SCC cycle ({} nodes): {}", members.len(), names.join(", "));
            }
        }

        // Expand: multi-member SCCs via optimal_scc_order, singletons directly
        scc_order
            .into_iter()
            .flat_map(|scc_node| {
                let members = &condensed[scc_node];
                if members.len() > 1 {
                    self.optimal_scc_order(members)
                } else {
                    vec![self.orig(members[0])]
                }
            })
            .collect()
    }

    /// Brute-force minimum-upward-permutation for SCC members.
    /// Enumerates all permutations and picks the one with the lowest sum of
    /// "upward" edge weights (edges from later → earlier in the permutation).
    /// Falls back to alphabetical for n > 8 (40320 permutations limit).
    fn optimal_scc_order(&self, members: &[LocalIdx]) -> Vec<NodeIndex> {
        let scc_size = members.len();

        // Guard: too many permutations → fallback alphabetical
        if scc_size > 8 {
            let mut result: Vec<NodeIndex> =
                members.iter().map(|&member| self.orig(member)).collect();
            result.sort_by_key(|&idx| (self.get_name)(idx));
            return result;
        }

        // Extract pairwise weights: weights[src][dst] = edge weight from members[src] to members[dst]
        let mut weights = vec![vec![0usize; scc_size]; scc_size];
        for src in 0..scc_size {
            for dst in 0..scc_size {
                if src != dst
                    && let Some(edge_idx) = self.subgraph.find_edge(members[src], members[dst])
                {
                    weights[src][dst] = self.subgraph[edge_idx];
                }
            }
        }

        // Enumerate all permutations, score by upward weight sum
        let mut best_perm: Vec<usize> = (0..scc_size).collect();
        let mut best_score = usize::MAX;
        let mut best_names: Vec<String> = Vec::new();

        for perm in (0..scc_size).permutations(scc_size) {
            let score: usize = (0..scc_size)
                .flat_map(|later| {
                    let weights = &weights;
                    let perm = &perm;
                    (0..later).map(move |earlier| weights[perm[later]][perm[earlier]])
                })
                .sum();

            let names: Vec<String> = perm.iter().map(|&i| self.name(members[i])).collect();
            if (score, &names) < (best_score, &best_names) {
                best_score = score;
                best_names = names;
                best_perm = perm;
            }
        }

        best_perm.iter().map(|&i| self.orig(members[i])).collect()
    }
}

/// Build a condensed DAG where each SCC becomes a single node.
/// Returns (condensed graph, mapping from original node → SCC node).
fn condense_sccs(
    graph: &DiGraph<NodeIndex, usize>,
    sccs: &[Vec<LocalIdx>],
) -> (DiGraph<Vec<LocalIdx>, ()>, HashMap<LocalIdx, LocalIdx>) {
    let mut condensed: DiGraph<Vec<LocalIdx>, ()> = DiGraph::new();
    let mut node_to_scc: HashMap<LocalIdx, LocalIdx> = HashMap::new();

    for scc in sccs {
        let scc_node = condensed.add_node(scc.clone());
        for &node in scc {
            node_to_scc.insert(node, scc_node);
        }
    }

    for edge in graph.edge_references() {
        let src_scc = node_to_scc[&edge.source()];
        let dst_scc = node_to_scc[&edge.target()];
        if src_scc != dst_scc && !condensed.contains_edge(src_scc, dst_scc) {
            condensed.add_edge(src_scc, dst_scc, ());
        }
    }

    (condensed, node_to_scc)
}

/// Kahn's topological sort with a generic priority tiebreaker.
fn kahns_toposort<N, E, P: Ord>(
    graph: &DiGraph<N, E>,
    priority: impl Fn(LocalIdx) -> P,
) -> Vec<LocalIdx> {
    let mut in_degree = vec![0usize; graph.node_count()];
    for node in graph.node_indices() {
        in_degree[node.index()] = graph.edges_directed(node, Direction::Incoming).count();
    }

    let mut heap: BinaryHeap<Reverse<(P, LocalIdx)>> = graph
        .node_indices()
        .filter(|&node| in_degree[node.index()] == 0)
        .map(|node| Reverse((priority(node), node)))
        .collect();

    let mut result = Vec::new();
    while let Some(Reverse((_, node))) = heap.pop() {
        result.push(node);
        for neighbor in graph.neighbors(node) {
            let degree = &mut in_degree[neighbor.index()];
            *degree -= 1;
            if *degree == 0 {
                heap.push(Reverse((priority(neighbor), neighbor)));
            }
        }
    }
    result
}

/// Float wrapper that implements `Eq`/`Ord` for use in `BinaryHeap`.
struct Score(f64);
impl PartialEq for Score {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for Score {}
impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Score {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a test digraph with named nodes and weighted edges.
    /// Node weights are `NodeIndex::new(10 + i)` to avoid overlap with local indices.
    fn test_graph(
        names: &[&str],
        edges: &[(usize, usize, usize)],
    ) -> (DiGraph<NodeIndex, usize>, Vec<String>) {
        let mut graph = DiGraph::new();
        let base = 10;
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        let locals: Vec<LocalIdx> = names
            .iter()
            .enumerate()
            .map(|(i, _)| graph.add_node(NodeIndex::new(base + i)))
            .collect();
        for &(from, to, weight) in edges {
            graph.add_edge(locals[from], locals[to], weight);
        }
        (graph, names)
    }

    /// Name lookup for test graphs with base offset 10.
    fn name_fn(names: &[String]) -> impl Fn(NodeIndex) -> String + '_ {
        move |idx| names[idx.index() - 10].clone()
    }

    /// Build a `ToposortGraph` from a test graph and name function.
    fn build_ctx<'a, F>(graph: &'a DiGraph<NodeIndex, usize>, get_name: F) -> ToposortGraph<'a, F> {
        ToposortGraph {
            node_to_orig: graph
                .node_indices()
                .map(|node| (node, graph[node]))
                .collect(),
            subgraph: graph,
            get_name,
        }
    }

    // === stable_toposort Cycle Tests ===

    #[test]
    fn test_stable_toposort_with_cycle_returns_sorted() {
        let (sibling_deps, names) = test_graph(&["a", "b"], &[(0, 1, 1), (1, 0, 1)]);
        let get_name = name_fn(&names);

        let result = stable_toposort(&sibling_deps, &get_name);

        assert_eq!(result.len(), 2, "Should return all nodes, not empty vec");
        let result_names: Vec<String> = result.iter().map(|&idx| get_name(idx)).collect();
        assert_eq!(result_names, vec!["a", "b"], "Alphabetical within SCC");
    }

    // === SCC Fallback Tests ===

    #[test]
    fn test_scc_fallback_three_node_cycle() {
        let (graph, names) = test_graph(&["a", "b", "c"], &[(0, 1, 1), (1, 2, 1), (2, 0, 1)]);
        let ctx = build_ctx(&graph, name_fn(&names));

        let result = ctx.scc_fallback_sort();
        let result_names: Vec<String> = result.iter().map(|&idx| (ctx.get_name)(idx)).collect();
        assert_eq!(
            result_names,
            vec!["a", "b", "c"],
            "SCC members sorted alphabetically"
        );
    }

    #[test]
    fn test_scc_fallback_with_external_node() {
        let (graph, names) = test_graph(&["a", "b", "c"], &[(0, 1, 1), (1, 0, 1), (2, 0, 1)]);
        let ctx = build_ctx(&graph, name_fn(&names));

        let result = ctx.scc_fallback_sort();
        let result_names: Vec<String> = result.iter().map(|&idx| (ctx.get_name)(idx)).collect();
        assert_eq!(
            result_names,
            vec!["c", "a", "b"],
            "Dependent C first, then SCC{{A,B}} alphabetically"
        );
    }

    // === Barycenter Direction Test ===

    #[test]
    fn test_barycenter_incoming_gives_dependents() {
        let mut g: DiGraph<NodeIndex, usize> = DiGraph::new();
        let a_orig = NodeIndex::new(0);
        let b_orig = NodeIndex::new(1);
        let a = g.add_node(a_orig);
        let b = g.add_node(b_orig);
        g.add_edge(a, b, 1);

        let incoming: Vec<_> = g.neighbors_directed(b, Direction::Incoming).collect();
        assert_eq!(incoming, vec![a], "Incoming neighbors of B should be [A]");
    }
}
