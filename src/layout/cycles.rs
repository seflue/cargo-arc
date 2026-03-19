//! Cycle detection for directed graphs using Johnson's algorithm.

use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

/// An elementary cycle in the module dependency graph (ordered path).
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    /// Ordered path of `NodeIndices` forming this elementary cycle.
    pub path: Vec<NodeIndex>,
}

impl Cycle {
    /// Iterate over the directed edges of this cycle.
    ///
    /// For a cycle `[A, B, C]` this yields `(A,B), (B,C), (C,A)`.
    #[allow(clippy::missing_panics_doc)]
    pub fn edges(&self) -> impl Iterator<Item = (NodeIndex, NodeIndex)> + '_ {
        self.path
            .windows(2)
            .map(|w| (w[0], w[1]))
            .chain(std::iter::once((*self.path.last().unwrap(), self.path[0])))
    }
}

/// Read-only graph data for one iteration of Johnson's least-vertex optimization.
struct JohnsonGraph<'a> {
    graph: &'a petgraph::graph::DiGraph<NodeIndex, ()>,
    active: HashSet<NodeIndex>,
}

impl JohnsonGraph<'_> {
    fn neighbors(&self, node: NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
        self.graph
            .edges(node)
            .map(|e| e.target())
            .filter(|n| self.active.contains(n))
    }
}

/// Mutable DFS state for Johnson's circuit-finding algorithm.
struct JohnsonState {
    start: NodeIndex,
    stack: Vec<NodeIndex>,
    blocked: HashSet<NodeIndex>,
    block_map: HashMap<NodeIndex, HashSet<NodeIndex>>,
    raw_cycles: Vec<Vec<NodeIndex>>,
    limit: usize,
}

impl JohnsonState {
    fn new(start: NodeIndex, limit: usize) -> Self {
        Self {
            start,
            stack: Vec::new(),
            blocked: HashSet::new(),
            block_map: HashMap::new(),
            raw_cycles: Vec::new(),
            limit,
        }
    }

    fn unblock(&mut self, node: NodeIndex) {
        if self.blocked.remove(&node)
            && let Some(dependents) = self.block_map.remove(&node)
        {
            for dep in dependents {
                self.unblock(dep);
            }
        }
    }

    fn circuit(&mut self, johnson: &JohnsonGraph, node: NodeIndex) -> bool {
        if self.raw_cycles.len() >= self.limit {
            return true;
        }

        let mut found_cycle = false;
        self.stack.push(node);
        self.blocked.insert(node);

        let neighbors: Vec<_> = johnson.neighbors(node).collect();
        for next in neighbors {
            if self.raw_cycles.len() >= self.limit {
                found_cycle = true;
                break;
            }
            if next == self.start {
                self.raw_cycles.push(self.stack.clone());
                found_cycle = true;
            } else if !self.blocked.contains(&next) && self.circuit(johnson, next) {
                found_cycle = true;
            }
        }

        if found_cycle {
            self.unblock(node);
        } else {
            let neighbors: Vec<_> = johnson.neighbors(node).collect();
            for next in neighbors {
                self.block_map.entry(next).or_default().insert(node);
            }
        }

        self.stack.pop();
        found_cycle
    }
}

/// Extension trait for Johnson's circuit-finding algorithm.
pub trait JohnsonCycles {
    /// Find elementary cycles using Johnson's algorithm, up to `limit`.
    ///
    /// Expects node weights to be the original `NodeIndex` values (e.g. produced
    /// by `filter_map`). Returns ordered paths — each cycle is a distinct
    /// elementary circuit, so overlapping cycles (e.g. B↔C + B↔D) produce
    /// separate entries.
    ///
    /// Stops searching after `limit` cycles are found. Pass `usize::MAX` for no limit.
    fn johnson_cycles(&self, limit: usize) -> Vec<Cycle>;
}

impl JohnsonCycles for petgraph::graph::DiGraph<NodeIndex, ()> {
    fn johnson_cycles(&self, limit: usize) -> Vec<Cycle> {
        let sorted_nodes = {
            let mut v: Vec<_> = self.node_indices().collect();
            v.sort_unstable_by_key(|n| n.index());
            v
        };

        let mut result = Vec::new();

        // Johnson's least-vertex optimization: for each start node, only nodes
        // at or after its position in sorted order are active.
        for (start_pos, &start) in sorted_nodes.iter().enumerate() {
            if result.len() >= limit {
                break;
            }
            let active = sorted_nodes[start_pos..].iter().copied().collect();
            let johnson = JohnsonGraph {
                graph: self,
                active,
            };
            let remaining = limit - result.len();
            let mut state = JohnsonState::new(start, remaining);
            state.circuit(&johnson, start);

            for raw in state.raw_cycles {
                result.push(Cycle {
                    path: raw.iter().map(|&node| self[node]).collect(),
                });
            }
        }

        result
    }
}

/// Cycle limit to prevent exponential blowup on large, densely connected graphs.
///
/// Johnson's algorithm enumerates ALL elementary cycles — output-sensitive at
/// O((n+e)(c+1)).  Graphs like wgpu (578 nodes, 2701 edges) can have billions
/// of overlapping cycles. This limit caps enumeration; a future cycle-basis
/// algorithm (ca-0340) will replace Johnson's entirely.
const CYCLE_LIMIT: usize = 10_000;

/// Extension trait for finding elementary cycles with SCC pre-filtering.
pub trait ElementaryCycles {
    /// Find elementary cycles, using Tarjan SCCs to prune acyclic subgraphs
    /// before running Johnson's algorithm on each component.
    ///
    /// Stops after [`CYCLE_LIMIT`] cycles to prevent exponential blowup
    /// on graphs with many overlapping cycles.
    fn elementary_cycles(&self) -> Vec<Cycle>;
}

impl ElementaryCycles for petgraph::graph::DiGraph<NodeIndex, ()> {
    fn elementary_cycles(&self) -> Vec<Cycle> {
        let mut result = Vec::new();
        for scc in tarjan_scc(self) {
            if scc.len() <= 1 {
                continue;
            }
            let remaining = CYCLE_LIMIT.saturating_sub(result.len());
            if remaining == 0 {
                eprintln!(
                    "warning: cycle detection stopped after {CYCLE_LIMIT} cycles \
                     (graph too dense for exhaustive enumeration)"
                );
                break;
            }
            let scc_set: HashSet<_> = scc.into_iter().collect();
            let sub = self.filter_map(
                |idx, &weight| scc_set.contains(&idx).then_some(weight),
                |_, ()| Some(()),
            );
            result.extend(sub.johnson_cycles(remaining));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Build a test digraph with `n` nodes and the given directed edges.
    fn digraph(
        node_count: usize,
        edges: &[(usize, usize)],
    ) -> petgraph::graph::DiGraph<NodeIndex, ()> {
        let mut g = petgraph::graph::DiGraph::new();
        (0..node_count).for_each(|i| {
            g.add_node(NodeIndex::new(i));
        });
        g.extend_with_edges(edges.iter().map(|&(from, to)| (node(from), node(to))));
        g
    }

    fn node(i: usize) -> NodeIndex {
        NodeIndex::new(i)
    }

    #[test]
    fn test_no_elementary_cycles() {
        // A -> B -> C (linear, no cycle)
        let graph = digraph(3, &[(0, 1), (1, 2)]);
        let cycles = graph.elementary_cycles();
        assert!(cycles.is_empty(), "Linear graph should have no cycles");
    }

    #[test]
    fn test_direct_elementary_cycle() {
        // A <-> B
        let graph = digraph(2, &[(0, 1), (1, 0)]);
        let cycles = graph.elementary_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].path.len(), 2);
        let nodes: HashSet<_> = cycles[0].path.iter().copied().collect();
        assert!(nodes.contains(&node(0)));
        assert!(nodes.contains(&node(1)));
    }

    #[test]
    fn test_transitive_elementary_cycle() {
        // A -> B -> C -> A
        let graph = digraph(3, &[(0, 1), (1, 2), (2, 0)]);
        let cycles = graph.elementary_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].path.len(), 3);
    }

    #[test]
    fn test_overlapping_elementary_cycles() {
        // B <-> C and B <-> D — two overlapping cycles sharing node B.
        // Tarjan SCC merges into one SCC {B, C, D}.
        // Johnson's should find 2 separate elementary cycles.
        let graph = digraph(3, &[(0, 1), (1, 0), (0, 2), (2, 0)]);
        let cycles = graph.elementary_cycles();
        assert_eq!(
            cycles.len(),
            2,
            "Should detect 2 overlapping elementary cycles, got {}",
            cycles.len()
        );

        for cycle in &cycles {
            assert_eq!(cycle.path.len(), 2);
        }

        let b_count = cycles.iter().filter(|c| c.path.contains(&node(0))).count();
        assert_eq!(b_count, 2, "B should participate in both cycles");
    }

    #[test]
    fn test_independent_elementary_cycles() {
        // A <-> B (cycle 1), C <-> D (cycle 2) — disjoint
        let graph = digraph(4, &[(0, 1), (1, 0), (2, 3), (3, 2)]);
        let cycles = graph.elementary_cycles();
        assert_eq!(cycles.len(), 2);

        let all_nodes: HashSet<_> = cycles.iter().flat_map(|c| c.path.iter().copied()).collect();
        assert!(all_nodes.contains(&node(0)));
        assert!(all_nodes.contains(&node(1)));
        assert!(all_nodes.contains(&node(2)));
        assert!(all_nodes.contains(&node(3)));
    }
}
