//! Minimal-cycle-per-edge detection for directed graphs.

use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{BTreeSet, HashMap, HashSet};

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

/// Result of the minimal-cycle-per-edge analysis.
pub struct CycleAnalysis {
    /// Deduplicated minimal cycles (one shortest cycle per edge, dedup'd by
    /// arc-set, each rotated to start at its smallest `NodeIndex`).
    pub cycles: Vec<Cycle>,
    /// For every cyclic edge: ascending indices into `cycles` it lies on.
    pub edge_cycles: HashMap<(NodeIndex, NodeIndex), Vec<usize>>,
}

/// Shortest cycle per edge, bounded and deterministic (replaces exhaustive
/// elementary-cycle enumeration).
pub trait MinimalCycles {
    /// For each edge inside a non-trivial SCC, compute the shortest cycle that
    /// carries it, then deduplicate by arc-set. `O(V·(V+E))` per SCC — no cap.
    ///
    /// Expects node weights to be the original `NodeIndex` values.
    fn minimal_cycles(&self) -> CycleAnalysis;
}

impl MinimalCycles for petgraph::graph::DiGraph<NodeIndex, ()> {
    fn minimal_cycles(&self) -> CycleAnalysis {
        let mut raw: Vec<Cycle> = Vec::new();

        for scc in tarjan_scc(self) {
            if scc.len() <= 1 {
                continue;
            }
            let scc_set: HashSet<NodeIndex> = scc.iter().copied().collect();

            let mut nodes = scc;
            nodes.sort_unstable_by_key(|n| n.index());

            for &v in &nodes {
                let parent = bfs_parents(self, v, &scc_set);

                // Every in-edge u->v of the SCC gets its shortest cycle v⇝u + u->v.
                let mut sources: Vec<NodeIndex> = self
                    .edges_directed(v, petgraph::Direction::Incoming)
                    .map(|e| e.source())
                    .filter(|u| scc_set.contains(u) && *u != v)
                    .collect();
                sources.sort_unstable_by_key(|n| n.index());

                for u in sources {
                    if let Some(local_path) = reconstruct(&parent, v, u) {
                        raw.push(Cycle {
                            path: local_path.iter().map(|&n| self[n]).collect(),
                        });
                    }
                }
            }
        }

        // Deduplicate by arc-set; keep one canonical rotation per distinct cycle.
        let mut seen: HashSet<BTreeSet<(NodeIndex, NodeIndex)>> = HashSet::new();
        let mut cycles: Vec<Cycle> = Vec::new();
        for cycle in raw {
            let arc_set: BTreeSet<(NodeIndex, NodeIndex)> = cycle.edges().collect();
            if seen.insert(arc_set) {
                cycles.push(Cycle {
                    path: canonical_rotation(cycle.path),
                });
            }
        }

        // edge_cycles: every edge of every kept cycle -> that cycle's index.
        let mut edge_cycles: HashMap<(NodeIndex, NodeIndex), Vec<usize>> = HashMap::new();
        for (idx, cycle) in cycles.iter().enumerate() {
            for edge in cycle.edges() {
                edge_cycles.entry(edge).or_default().push(idx);
            }
        }

        CycleAnalysis {
            cycles,
            edge_cycles,
        }
    }
}

/// Level-synchronous forward BFS from `source`, restricted to `scc`. Returns a
/// parent pointer per reached node (`source`'s parent is itself). Each frontier
/// is processed in ascending index order, so a node's parent is the
/// smallest-index predecessor at the shortest distance — making reconstruction
/// deterministic.
fn bfs_parents(
    graph: &petgraph::graph::DiGraph<NodeIndex, ()>,
    source: NodeIndex,
    scc: &HashSet<NodeIndex>,
) -> HashMap<NodeIndex, NodeIndex> {
    let mut parent: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    parent.insert(source, source);
    let mut frontier = vec![source];
    while !frontier.is_empty() {
        frontier.sort_unstable_by_key(|n| n.index());
        let mut next = Vec::new();
        for u in frontier {
            let mut targets: Vec<NodeIndex> = graph
                .edges(u)
                .map(|e| e.target())
                .filter(|w| scc.contains(w))
                .collect();
            targets.sort_unstable_by_key(|n| n.index());
            for w in targets {
                if let std::collections::hash_map::Entry::Vacant(slot) = parent.entry(w) {
                    slot.insert(u);
                    next.push(w);
                }
            }
        }
        frontier = next;
    }
    parent
}

/// Reconstruct shortest path `source ⇝ target` from a `bfs_parents(source)` map.
/// Returns `[source, …, target]`, or `None` if `target` was unreachable.
fn reconstruct(
    parent: &HashMap<NodeIndex, NodeIndex>,
    source: NodeIndex,
    target: NodeIndex,
) -> Option<Vec<NodeIndex>> {
    parent.get(&target)?;
    let mut path = vec![target];
    let mut current = target;
    while current != source {
        current = parent[&current];
        path.push(current);
    }
    path.reverse();
    Some(path)
}

/// Rotate a cycle path to start at its smallest `NodeIndex`, preserving
/// direction — a stable representative for display.
fn canonical_rotation(mut path: Vec<NodeIndex>) -> Vec<NodeIndex> {
    if let Some(min_pos) = (0..path.len()).min_by_key(|&i| path[i].index()) {
        path.rotate_left(min_pos);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn minimal_no_cycles() {
        let graph = digraph(3, &[(0, 1), (1, 2)]);
        let a = graph.minimal_cycles();
        assert!(a.cycles.is_empty());
        assert!(a.edge_cycles.is_empty());
    }

    #[test]
    fn minimal_direct_cycle() {
        let graph = digraph(2, &[(0, 1), (1, 0)]);
        let a = graph.minimal_cycles();
        assert_eq!(a.cycles.len(), 1);
        assert_eq!(a.cycles[0].path.len(), 2);
    }

    #[test]
    fn minimal_transitive_cycle() {
        let graph = digraph(3, &[(0, 1), (1, 2), (2, 0)]);
        let a = graph.minimal_cycles();
        assert_eq!(a.cycles.len(), 1);
        assert_eq!(a.cycles[0].path.len(), 3);
    }

    #[test]
    fn minimal_overlapping_cycles() {
        // 0<->1 and 0<->2 — one SCC, two disjoint minimal cycles.
        let graph = digraph(3, &[(0, 1), (1, 0), (0, 2), (2, 0)]);
        let a = graph.minimal_cycles();
        assert_eq!(a.cycles.len(), 2);
        for c in &a.cycles {
            assert_eq!(c.path.len(), 2);
        }
    }

    #[test]
    fn minimal_independent_cycles() {
        let graph = digraph(4, &[(0, 1), (1, 0), (2, 3), (3, 2)]);
        let a = graph.minimal_cycles();
        assert_eq!(a.cycles.len(), 2);
    }

    #[test]
    fn minimal_prefers_shortcut_over_triangle() {
        // Triangle 0->1->2->0 plus a 2-cycle 0<->2. The minimal cycle carrying
        // edge 0->2 must be the length-2 loop, not the triangle.
        let graph = digraph(3, &[(0, 1), (1, 2), (2, 0), (0, 2)]);
        let a = graph.minimal_cycles();
        let idxs = &a.edge_cycles[&(node(0), node(2))];
        assert!(
            idxs.iter().any(|&i| a.cycles[i].path.len() == 2),
            "edge 0->2 should lie on a length-2 minimal cycle"
        );
    }

    #[test]
    fn minimal_shared_edge_lists_all_cycles() {
        // Two triangles sharing directed edge 0->1:
        //   0->1->2->0  and  0->1->3->0
        let graph = digraph(4, &[(0, 1), (1, 2), (2, 0), (1, 3), (3, 0)]);
        let a = graph.minimal_cycles();
        assert_eq!(a.cycles.len(), 2);
        assert_eq!(a.edge_cycles[&(node(0), node(1))].len(), 2);
    }

    #[test]
    fn minimal_covers_every_scc_edge() {
        let graph = digraph(3, &[(0, 1), (1, 2), (2, 0)]);
        let a = graph.minimal_cycles();
        for e in [(node(0), node(1)), (node(1), node(2)), (node(2), node(0))] {
            assert!(a.edge_cycles.contains_key(&e), "edge {e:?} not covered");
        }
    }

    #[test]
    fn minimal_is_deterministic() {
        let edges = &[(0, 1), (1, 2), (2, 0), (1, 3), (3, 0)];
        let a = digraph(4, edges).minimal_cycles();
        let b = digraph(4, edges).minimal_cycles();
        assert_eq!(a.cycles, b.cycles);
    }
}
