//! Representative-cycle detection for directed graphs.

use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{BTreeSet, HashMap, HashSet};

/// A cycle in the module dependency graph: a closed sequence of nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    /// The nodes of this cycle in order. The closing edge back to the first
    /// node is implied, not stored.
    pub nodes: Vec<NodeIndex>,
}

impl Cycle {
    /// Iterate over the directed edges of this cycle.
    ///
    /// For a cycle `[A, B, C]` this yields `(A,B), (B,C), (C,A)`.
    #[allow(clippy::missing_panics_doc)]
    pub fn edges(&self) -> impl Iterator<Item = (NodeIndex, NodeIndex)> + '_ {
        self.nodes
            .windows(2)
            .map(|w| (w[0], w[1]))
            .chain(std::iter::once((
                *self.nodes.last().unwrap(),
                self.nodes[0],
            )))
    }
}

/// Result of the representative-cycle analysis.
pub struct CycleAnalysis {
    /// Deduplicated representative cycles (one shortest cycle per edge, dedup'd by
    /// arc-set, each rotated to start at its smallest `NodeIndex`).
    pub cycles: Vec<Cycle>,
    /// For every cyclic edge: ascending indices into `cycles` it lies on.
    pub edge_cycles: HashMap<(NodeIndex, NodeIndex), Vec<usize>>,
    /// For every node in a non-trivial SCC: its SCC id. Nodes outside any cycle
    /// are absent. Ids are assigned sequentially in `tarjan_scc` order.
    pub node_scc: HashMap<NodeIndex, usize>,
}

/// Shortest cycle per edge, bounded and deterministic (replaces exhaustive
/// elementary-cycle enumeration).
pub trait RepresentativeCycles {
    /// For each edge inside a non-trivial SCC, compute the shortest cycle that
    /// carries it, then deduplicate by arc-set. `O(V·(V+E))` per SCC — no cap.
    ///
    /// Expects node weights to be the original `NodeIndex` values.
    fn representative_cycles(&self) -> CycleAnalysis;
}

impl CycleAnalysis {
    /// Keep only the cycles `keep` accepts, remapping the indices in
    /// `edge_cycles` to the new positions.
    pub fn retain_cycles(&mut self, mut keep: impl FnMut(&Cycle) -> bool) {
        let mut remap: HashMap<usize, usize> = HashMap::new();
        let mut kept: Vec<Cycle> = Vec::new();
        for (old_idx, cycle) in self.cycles.drain(..).enumerate() {
            if keep(&cycle) {
                remap.insert(old_idx, kept.len());
                kept.push(cycle);
            }
        }
        self.cycles = kept;

        self.edge_cycles.retain(|_, idxs| {
            idxs.retain_mut(|i| match remap.get(i) {
                Some(&new_idx) => {
                    *i = new_idx;
                    true
                }
                None => false,
            });
            !idxs.is_empty()
        });
    }
}

impl RepresentativeCycles for petgraph::graph::DiGraph<NodeIndex, ()> {
    fn representative_cycles(&self) -> CycleAnalysis {
        let mut raw: Vec<Cycle> = Vec::new();
        let mut node_scc: HashMap<NodeIndex, usize> = HashMap::new();
        let mut next_scc_id = 0;

        for scc in tarjan_scc(self) {
            if scc.len() <= 1 {
                continue;
            }
            let scc_id = next_scc_id;
            next_scc_id += 1;
            for &node in &scc {
                node_scc.insert(self[node], scc_id);
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
                            nodes: local_path.iter().map(|&n| self[n]).collect(),
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
                    nodes: canonical_rotation(cycle.nodes),
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
            node_scc,
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

/// Rotate a cycle's node sequence to start at its smallest `NodeIndex`,
/// preserving direction — a stable starting point for display.
fn canonical_rotation(mut nodes: Vec<NodeIndex>) -> Vec<NodeIndex> {
    if let Some(min_pos) = (0..nodes.len()).min_by_key(|&i| nodes[i].index()) {
        nodes.rotate_left(min_pos);
    }
    nodes
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
    fn representative_no_cycles() {
        let graph = digraph(3, &[(0, 1), (1, 2)]);
        let a = graph.representative_cycles();
        assert!(a.cycles.is_empty());
        assert!(a.edge_cycles.is_empty());
        assert!(a.node_scc.is_empty());
    }

    #[test]
    fn representative_direct_cycle() {
        let graph = digraph(2, &[(0, 1), (1, 0)]);
        let a = graph.representative_cycles();
        assert_eq!(a.cycles.len(), 1);
        assert_eq!(a.cycles[0].nodes.len(), 2);
    }

    #[test]
    fn representative_transitive_cycle() {
        let graph = digraph(3, &[(0, 1), (1, 2), (2, 0)]);
        let a = graph.representative_cycles();
        assert_eq!(a.cycles.len(), 1);
        assert_eq!(a.cycles[0].nodes.len(), 3);
    }

    #[test]
    fn representative_overlapping_cycles() {
        // 0<->1 and 0<->2 — one SCC, two disjoint representative cycles.
        let graph = digraph(3, &[(0, 1), (1, 0), (0, 2), (2, 0)]);
        let a = graph.representative_cycles();
        assert_eq!(a.cycles.len(), 2);
        for c in &a.cycles {
            assert_eq!(c.nodes.len(), 2);
        }
        // All three nodes belong to the same SCC.
        let scc = a.node_scc[&node(0)];
        assert_eq!(a.node_scc[&node(1)], scc);
        assert_eq!(a.node_scc[&node(2)], scc);
    }

    #[test]
    fn representative_independent_cycles() {
        let graph = digraph(4, &[(0, 1), (1, 0), (2, 3), (3, 2)]);
        let a = graph.representative_cycles();
        assert_eq!(a.cycles.len(), 2);
        // Two disjoint SCCs {0,1} and {2,3} get distinct ids; each pair shares one.
        assert_eq!(a.node_scc[&node(0)], a.node_scc[&node(1)]);
        assert_eq!(a.node_scc[&node(2)], a.node_scc[&node(3)]);
        assert_ne!(a.node_scc[&node(0)], a.node_scc[&node(2)]);
    }

    #[test]
    fn representative_prefers_shortcut_over_triangle() {
        // Triangle 0->1->2->0 plus a 2-cycle 0<->2. The representative cycle carrying
        // edge 0->2 must be the length-2 loop, not the triangle.
        let graph = digraph(3, &[(0, 1), (1, 2), (2, 0), (0, 2)]);
        let a = graph.representative_cycles();
        let idxs = &a.edge_cycles[&(node(0), node(2))];
        assert!(
            idxs.iter().any(|&i| a.cycles[i].nodes.len() == 2),
            "edge 0->2 should lie on a length-2 representative cycle"
        );
    }

    #[test]
    fn representative_shared_edge_lists_all_cycles() {
        // Two triangles sharing directed edge 0->1:
        //   0->1->2->0  and  0->1->3->0
        let graph = digraph(4, &[(0, 1), (1, 2), (2, 0), (1, 3), (3, 0)]);
        let a = graph.representative_cycles();
        assert_eq!(a.cycles.len(), 2);
        assert_eq!(a.edge_cycles[&(node(0), node(1))].len(), 2);
    }

    #[test]
    fn representative_covers_every_scc_edge() {
        let graph = digraph(3, &[(0, 1), (1, 2), (2, 0)]);
        let a = graph.representative_cycles();
        for e in [(node(0), node(1)), (node(1), node(2)), (node(2), node(0))] {
            assert!(a.edge_cycles.contains_key(&e), "edge {e:?} not covered");
        }
    }

    #[test]
    fn retain_cycles_keeping_all_leaves_analysis_unchanged() {
        let graph = digraph(4, &[(0, 1), (1, 2), (2, 0), (1, 3), (3, 0)]);
        let mut a = graph.representative_cycles();
        let before_cycles = a.cycles.clone();
        let before_edge_cycles = a.edge_cycles.clone();
        let before_node_scc = a.node_scc.clone();

        a.retain_cycles(|_| true);

        assert_eq!(a.cycles, before_cycles);
        assert_eq!(a.edge_cycles, before_edge_cycles);
        assert_eq!(a.node_scc, before_node_scc);
    }

    #[test]
    fn retain_cycles_drops_filtered_cycle_and_remaps_survivors() {
        // Two disjoint 2-cycles sharing node 0: keep only the one through node 3.
        let graph = digraph(4, &[(0, 1), (1, 0), (0, 3), (3, 0)]);
        let mut a = graph.representative_cycles();
        assert_eq!(a.cycles.len(), 2);
        let kept_nodes = a
            .cycles
            .iter()
            .find(|c| c.nodes.contains(&node(3)))
            .unwrap()
            .nodes
            .clone();

        a.retain_cycles(|c| c.nodes.contains(&node(3)));

        assert_eq!(a.cycles.len(), 1);
        assert_eq!(a.cycles[0].nodes, kept_nodes);
        for idxs in a.edge_cycles.values() {
            for &i in idxs {
                assert!(i < a.cycles.len(), "index {i} out of bounds");
                assert_eq!(a.cycles[i].nodes, kept_nodes);
            }
        }
    }

    #[test]
    fn retain_cycles_drops_edge_that_only_carried_the_filtered_cycle() {
        let graph = digraph(4, &[(0, 1), (1, 0), (0, 3), (3, 0)]);
        let mut a = graph.representative_cycles();
        assert!(a.edge_cycles.contains_key(&(node(0), node(1))));

        a.retain_cycles(|c| c.nodes.contains(&node(3)));

        assert!(!a.edge_cycles.contains_key(&(node(0), node(1))));
        assert!(!a.edge_cycles.contains_key(&(node(1), node(0))));
    }

    #[test]
    fn retain_cycles_dropping_all_clears_cycles_and_edge_cycles_but_keeps_node_scc() {
        let graph = digraph(2, &[(0, 1), (1, 0)]);
        let mut a = graph.representative_cycles();
        let before_node_scc = a.node_scc.clone();

        a.retain_cycles(|_| false);

        assert!(a.cycles.is_empty());
        assert!(a.edge_cycles.is_empty());
        assert_eq!(a.node_scc, before_node_scc);
    }

    #[test]
    fn representative_is_deterministic() {
        let edges = &[(0, 1), (1, 2), (2, 0), (1, 3), (3, 0)];
        let a = digraph(4, edges).representative_cycles();
        let b = digraph(4, edges).representative_cycles();
        assert_eq!(a.cycles, b.cycles);
    }
}
