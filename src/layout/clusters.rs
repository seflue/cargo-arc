//! Cluster-level cycle aggregation with guaranteed cut-sets.
//!
//! Aggregates the per-edge minimal cycles ([`CycleAnalysis`]) into
//! strongly-connected clusters and, per cluster, computes a cut-set whose
//! removal is proven to break every cycle: greedy set-cover over the minimal
//! cycles, then a single `tarjan_scc` verification with a residual re-cover loop

use super::cycles::{CycleAnalysis, MinimalCycles};
use crate::graph::{ArcGraph, Edge};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

/// One edge in a cluster's cut-set.
pub struct Cut {
    pub from: NodeIndex,
    pub to: NodeIndex,
    /// Gross cycles through this edge within the cluster (order-independent).
    pub breaks: usize,
    /// Reference sites on the edge (secondary effort proxy for ranking).
    pub refs: usize,
}

/// One strongly-connected cluster of mutually cyclic modules.
pub struct Cluster {
    /// Owning crate node (module cycles are always intra-crate).
    pub crate_idx: NodeIndex,
    /// All SCC member modules.
    pub nodes: Vec<NodeIndex>,
    /// Indices into [`CycleAnalysis::cycles`] contained in this cluster.
    pub cycles: Vec<usize>,
    /// Cut-set whose removal breaks every cycle, ranked best-first.
    pub cuts: Vec<Cut>,
}

/// Report over all cyclic clusters, ordered by cut-set size ascending.
pub struct ClusterReport {
    pub clusters: Vec<Cluster>,
}

/// How willing the cut tie-break is to cut an edge, given the direction it runs
/// through the module tree. Ordered worst-to-best cut candidate.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum CutBias {
    /// Parent → child (`mod x;`, `pub use x::Y`). Spelling out the module tree
    /// is not a dependency anyone can remove.
    Structural,
    /// Neither module is the other's direct parent.
    Neutral,
    /// Child → parent (`use super::Thing`), no re-export. The direction a
    /// developer can plausibly break.
    Preferred,
}

impl ArcGraph {
    /// Aggregate `analysis` into SCC clusters, each with a proven cut-set.
    ///
    /// `analysis` must come from `self.cycle_subgraph(include_reexports).minimal_cycles()`
    /// so that its `NodeIndex` values address nodes in `self` and the cut-sets are
    /// computed over the same subgraph.
    #[must_use]
    pub fn cluster_report(
        &self,
        analysis: &CycleAnalysis,
        include_reexports: bool,
    ) -> ClusterReport {
        let sub = self.cycle_subgraph(include_reexports);

        // Map each node of a non-trivial SCC to its cluster id.
        let mut node_scc: HashMap<NodeIndex, usize> = HashMap::new();
        let mut scc_members: Vec<Vec<NodeIndex>> = Vec::new();
        for comp in tarjan_scc(&sub) {
            if comp.len() <= 1 {
                continue;
            }
            let id = scc_members.len();
            let members: Vec<NodeIndex> = comp.iter().map(|&n| sub[n]).collect();
            for &orig in &members {
                node_scc.insert(orig, id);
            }
            scc_members.push(members);
        }

        // Group cycle indices by cluster (all nodes of a cycle share one SCC).
        let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); scc_members.len()];
        for (cycle_idx, cycle) in analysis.cycles.iter().enumerate() {
            if let Some(&id) = node_scc.get(&cycle.path[0]) {
                grouped[id].push(cycle_idx);
            }
        }

        let mut clusters: Vec<Cluster> = Vec::new();
        for (id, nodes) in scc_members.into_iter().enumerate() {
            let cycles = std::mem::take(&mut grouped[id]);
            if cycles.is_empty() {
                continue;
            }
            let cuts = self.cluster_cut_set(&sub, &nodes, &cycles, analysis);
            let crate_idx = self.owning_crate(nodes[0]);
            clusters.push(Cluster {
                crate_idx,
                nodes,
                cycles,
                cuts,
            });
        }

        // Default ordering: fewest cuts first, then deterministic tiebreaks.
        clusters.sort_by_key(cluster_sort_key);
        ClusterReport { clusters }
    }

    /// Greedy set-cover cut-set for one cluster, verified acyclic.
    fn cluster_cut_set(
        &self,
        sub: &DiGraph<NodeIndex, ()>,
        nodes: &[NodeIndex],
        cycle_idxs: &[usize],
        analysis: &CycleAnalysis,
    ) -> Vec<Cut> {
        let node_set: HashSet<NodeIndex> = nodes.iter().copied().collect();
        let in_cluster: HashSet<usize> = cycle_idxs.iter().copied().collect();

        // Edge -> gross cycles through it within this cluster (the "breaks" count).
        let mut gross: HashMap<(NodeIndex, NodeIndex), usize> = HashMap::new();
        let mut edge_cycles: HashMap<(NodeIndex, NodeIndex), HashSet<usize>> = HashMap::new();
        for (&edge, cyc_list) in &analysis.edge_cycles {
            let here: HashSet<usize> = cyc_list
                .iter()
                .copied()
                .filter(|c| in_cluster.contains(c))
                .collect();
            if here.is_empty() {
                continue;
            }
            gross.insert(edge, here.len());
            edge_cycles.insert(edge, here);
        }

        let mut chosen: Vec<(NodeIndex, NodeIndex)> = Vec::new();
        let mut open = in_cluster;
        self.greedy_cover(&edge_cycles, &mut open, &mut chosen);

        // Prove the cut-set acyclic with a single tarjan_scc pass; only when a
        // residual SCC survives (rare) enumerate its cycles and re-cover it. The
        // minimal-cycle cover alone does not guarantee acyclicity.
        let mut removed: HashSet<(NodeIndex, NodeIndex)> = chosen.iter().copied().collect();
        loop {
            let pruned = restricted_subgraph(sub, &node_set, &removed);
            if tarjan_scc(&pruned).iter().all(|scc| scc.len() <= 1) {
                break;
            }
            let residual = pruned.minimal_cycles();
            let residual_edges: HashMap<(NodeIndex, NodeIndex), HashSet<usize>> = residual
                .edge_cycles
                .iter()
                .map(|(&e, cs)| (e, cs.iter().copied().collect()))
                .collect();
            let mut residual_open: HashSet<usize> = (0..residual.cycles.len()).collect();
            let before = chosen.len();
            self.greedy_cover(&residual_edges, &mut residual_open, &mut chosen);
            for &e in &chosen[before..] {
                removed.insert(e);
            }
        }

        let mut cuts: Vec<Cut> = chosen
            .into_iter()
            .map(|(from, to)| Cut {
                from,
                to,
                breaks: gross.get(&(from, to)).copied().unwrap_or(0),
                refs: self.edge_refs(from, to),
            })
            .collect();
        // Rank: traffic desc, refs asc, name asc.
        cuts.sort_by(|a, b| {
            Reverse(a.breaks)
                .cmp(&Reverse(b.breaks))
                .then(a.refs.cmp(&b.refs))
                .then_with(|| {
                    self.qualified_name(a.from)
                        .cmp(&self.qualified_name(b.from))
                })
                .then_with(|| self.qualified_name(a.to).cmp(&self.qualified_name(b.to)))
        });
        cuts
    }

    /// Repeatedly remove the edge covering the most still-open cycles until none
    /// remain, appending each pick to `chosen`. Ties break on [`CutBias`], then
    /// fewer refs, then smaller name, so the choice is deterministic.
    fn greedy_cover(
        &self,
        edge_cycles: &HashMap<(NodeIndex, NodeIndex), HashSet<usize>>,
        open: &mut HashSet<usize>,
        chosen: &mut Vec<(NodeIndex, NodeIndex)>,
    ) {
        let mut coverage = edge_cycles.clone();
        while !open.is_empty() {
            let best = coverage
                .iter()
                .filter(|(_, cs)| !cs.is_empty())
                .max_by(|(ea, ca), (eb, cb)| {
                    ca.len()
                        .cmp(&cb.len())
                        // module-tree direction outranks ref count
                        .then_with(|| self.cut_bias(**ea).cmp(&self.cut_bias(**eb)))
                        // fewer refs is better, so the lower-ref edge ranks greater
                        .then_with(|| self.edge_refs(eb.0, eb.1).cmp(&self.edge_refs(ea.0, ea.1)))
                        // lexicographically smaller name is better
                        .then_with(|| self.name_key(**eb).cmp(&self.name_key(**ea)))
                })
                .map(|(&e, _)| e);
            let Some(edge) = best else { break };

            let covered = coverage.remove(&edge).unwrap_or_default();
            for c in &covered {
                open.remove(c);
            }
            for cs in coverage.values_mut() {
                for c in &covered {
                    cs.remove(c);
                }
            }
            chosen.push(edge);
        }
    }

    /// Reference sites carried by the module-dependency edge `from -> to`.
    fn edge_refs(&self, from: NodeIndex, to: NodeIndex) -> usize {
        self.find_edge(from, to)
            .and_then(|e| match &self[e] {
                Edge::ModuleDep { locations, .. } => Some(locations.len()),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn name_key(&self, edge: (NodeIndex, NodeIndex)) -> (String, String) {
        (self.qualified_name(edge.0), self.qualified_name(edge.1))
    }

    /// Which way the edge runs through the module tree, as the cut tie-break
    /// sees it.
    ///
    /// A child→parent edge that is itself a pure re-export (the prelude
    /// pattern, `pub use super::*;`) ranks [`Structural`](CutBias::Structural)
    /// rather than [`Preferred`](CutBias::Preferred): under the default graph
    /// this edge doesn't exist at all (ADR-022 drops it as non-coupling), so
    /// it only shows up as a cut candidate under `--include-reexports`, where
    /// it's still just a facade, not a layer to break.
    fn cut_bias(&self, edge: (NodeIndex, NodeIndex)) -> CutBias {
        let (from, to) = edge;
        if self.contains_child(from, to) {
            return CutBias::Structural;
        }
        if self.contains_child(to, from) {
            return if self
                .find_edge(from, to)
                .is_some_and(|e| self[e].is_reexport_module_dep())
            {
                CutBias::Structural
            } else {
                CutBias::Preferred
            };
        }
        CutBias::Neutral
    }
}

fn cluster_sort_key(c: &Cluster) -> (usize, usize, usize, usize) {
    let min_node = c.nodes.iter().map(|n| n.index()).min().unwrap_or(0);
    (c.cuts.len(), c.cycles.len(), c.nodes.len(), min_node)
}

/// Copy of `sub` restricted to `node_set`, with `removed` edges dropped. Node
/// weights stay the original `NodeIndex` values so `minimal_cycles` on the
/// result reports cycles in `self`'s index space.
fn restricted_subgraph(
    sub: &DiGraph<NodeIndex, ()>,
    node_set: &HashSet<NodeIndex>,
    removed: &HashSet<(NodeIndex, NodeIndex)>,
) -> DiGraph<NodeIndex, ()> {
    let mut g = DiGraph::new();
    let mut map: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    for n in sub.node_indices() {
        let orig = sub[n];
        if node_set.contains(&orig) {
            map.insert(orig, g.add_node(orig));
        }
    }
    for e in sub.edge_references() {
        let a = sub[e.source()];
        let b = sub[e.target()];
        if node_set.contains(&a) && node_set.contains(&b) && !removed.contains(&(a, b)) {
            g.add_edge(map[&a], map[&b], ());
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ArcGraph, Edge, Node};
    use crate::model::{EdgeContext, SourceLocation};

    /// Single-crate graph: `modules` by name, production `ModuleDep` edges
    /// `(from, to, ref_count)`. Returns the graph and the module node indices.
    fn graph_with(modules: &[&str], deps: &[(usize, usize, usize)]) -> (ArcGraph, Vec<NodeIndex>) {
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
                g.add_edge(crate_idx, n, Edge::Contains);
                n
            })
            .collect();
        for &(from, to, refs) in deps {
            let locations = (0..refs)
                .map(|i| SourceLocation {
                    file: format!("src/{}.rs", modules[from]).into(),
                    line: i + 1,
                    symbols: vec![],
                    module_path: String::new(),
                    via_reexport: false,
                })
                .collect();
            g.add_edge(
                idx[from],
                idx[to],
                Edge::ModuleDep {
                    locations,
                    context: EdgeContext::production(),
                },
            );
        }
        (g, idx)
    }

    fn report(g: &ArcGraph) -> ClusterReport {
        let analysis = g.production_subgraph().minimal_cycles();
        g.cluster_report(&analysis, true)
    }

    /// Assert that removing `cuts` leaves the cluster `nodes` acyclic.
    fn assert_acyclic_after_cuts(g: &ArcGraph, nodes: &[NodeIndex], cuts: &[Cut]) {
        let sub = g.production_subgraph();
        let node_set: HashSet<NodeIndex> = nodes.iter().copied().collect();
        let removed: HashSet<(NodeIndex, NodeIndex)> =
            cuts.iter().map(|c| (c.from, c.to)).collect();
        let left = restricted_subgraph(&sub, &node_set, &removed).minimal_cycles();
        assert!(
            left.cycles.is_empty(),
            "cut-set did not break all cycles: {} remain",
            left.cycles.len()
        );
    }

    #[test]
    fn acyclic_graph_yields_no_clusters() {
        let (g, _) = graph_with(&["a", "b", "c"], &[(0, 1, 1), (1, 2, 1)]);
        assert!(report(&g).clusters.is_empty());
    }

    #[test]
    fn child_to_parent_edge_is_cut_even_with_more_refs() {
        // `a` is the parent module, `b` is nested inside it. `a -> b` is a
        // re-export (`pub use b::X`, 2 refs), `b -> a` is a plain import
        // (`use super::Y`, 5 refs). The module-tree prior must still pick
        // the child->parent edge, even though it carries more refs.
        let mut g = ArcGraph::new();
        let crate_idx = g.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let a = g.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let b = g.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        g.add_edge(crate_idx, a, Edge::Contains);
        g.add_edge(a, b, Edge::Contains);

        let reexport_locations = (0..2)
            .map(|i| SourceLocation {
                file: "src/a.rs".into(),
                line: i + 1,
                symbols: vec![],
                module_path: String::new(),
                via_reexport: true,
            })
            .collect();
        g.add_edge(
            a,
            b,
            Edge::ModuleDep {
                locations: reexport_locations,
                context: EdgeContext::production(),
            },
        );

        let plain_locations = (0..5)
            .map(|i| SourceLocation {
                file: "src/b.rs".into(),
                line: i + 1,
                symbols: vec![],
                module_path: String::new(),
                via_reexport: false,
            })
            .collect();
        g.add_edge(
            b,
            a,
            Edge::ModuleDep {
                locations: plain_locations,
                context: EdgeContext::production(),
            },
        );

        let r = report(&g);
        assert_eq!(r.clusters.len(), 1);
        let c = &r.clusters[0];
        assert_eq!(c.cycles.len(), 1);
        assert_eq!(c.cuts.len(), 1);
        let cut = &c.cuts[0];
        assert_eq!((cut.from, cut.to), (b, a));
        assert_eq!(cut.refs, 5);
        assert_acyclic_after_cuts(&g, &c.nodes, &c.cuts);
    }

    #[test]
    fn reexport_child_to_parent_edge_is_not_preferentially_cut() {
        // `a` is the parent module, `b` is nested inside it and re-exports
        // `a`'s items (`pub use super::*;`, the prelude pattern) via `b -> a`,
        // 1 ref. `c` is an unrelated module, forming the cycle a->c->b->a
        // with plain, non-reexport edges a->c (3 refs) and c->b (5 refs).
        // Even though `b -> a` carries the fewest refs, the module-tree prior
        // must not treat it as a preferred cut: it's structural, same as any
        // other re-export.
        let mut g = ArcGraph::new();
        let crate_idx = g.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let a = g.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let b = g.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        let c = g.add_node(Node::Module {
            name: "c".into(),
            crate_idx,
        });
        g.add_edge(crate_idx, a, Edge::Contains);
        g.add_edge(a, b, Edge::Contains);
        g.add_edge(crate_idx, c, Edge::Contains);

        let reexport_locations = (0..1)
            .map(|i| SourceLocation {
                file: "src/b.rs".into(),
                line: i + 1,
                symbols: vec![],
                module_path: String::new(),
                via_reexport: true,
            })
            .collect();
        g.add_edge(
            b,
            a,
            Edge::ModuleDep {
                locations: reexport_locations,
                context: EdgeContext::production(),
            },
        );

        let make_locations = |file: &str, count: usize| {
            (0..count)
                .map(|i| SourceLocation {
                    file: file.into(),
                    line: i + 1,
                    symbols: vec![],
                    module_path: String::new(),
                    via_reexport: false,
                })
                .collect()
        };
        g.add_edge(
            a,
            c,
            Edge::ModuleDep {
                locations: make_locations("src/a.rs", 3),
                context: EdgeContext::production(),
            },
        );
        g.add_edge(
            c,
            b,
            Edge::ModuleDep {
                locations: make_locations("src/c.rs", 5),
                context: EdgeContext::production(),
            },
        );

        let r = report(&g);
        assert_eq!(r.clusters.len(), 1);
        let clu = &r.clusters[0];
        assert_eq!(clu.cycles.len(), 1);
        assert_eq!(clu.cuts.len(), 1);
        let cut = &clu.cuts[0];
        assert_eq!((cut.from, cut.to), (a, c));
        assert_eq!(cut.refs, 3);
        assert_acyclic_after_cuts(&g, &clu.nodes, &clu.cuts);
    }

    #[test]
    fn fewer_refs_wins_without_a_parent_child_relation() {
        // a <-> b, siblings under the crate (no Contains edge between them).
        // With no module-tree prior to apply, the ref-count tie-break still
        // decides: a->b carries 2 refs, b->a carries 5.
        let (g, idx) = graph_with(&["a", "b"], &[(0, 1, 2), (1, 0, 5)]);
        let r = report(&g);
        assert_eq!(r.clusters.len(), 1);
        let c = &r.clusters[0];
        assert_eq!(c.cycles.len(), 1);
        assert_eq!(c.cuts.len(), 1);
        let cut = &c.cuts[0];
        assert_eq!((cut.from, cut.to), (idx[0], idx[1]));
        assert_eq!(cut.refs, 2);
        assert_eq!(cut.breaks, 1);
        assert_acyclic_after_cuts(&g, &c.nodes, &c.cuts);
    }

    #[test]
    fn shared_edge_is_ranked_by_traffic() {
        // Two triangles sharing directed edge 0->1:
        //   0->1->2->0  and  0->1->3->0. Removing 0->1 breaks both.
        let (g, idx) = graph_with(
            &["m0", "m1", "m2", "m3"],
            &[(0, 1, 1), (1, 2, 1), (2, 0, 1), (1, 3, 1), (3, 0, 1)],
        );
        let r = report(&g);
        assert_eq!(r.clusters.len(), 1);
        let c = &r.clusters[0];
        assert_eq!(c.cycles.len(), 2);
        assert_eq!(c.cuts.len(), 1);
        let cut = &c.cuts[0];
        assert_eq!((cut.from, cut.to), (idx[0], idx[1]));
        assert_eq!(cut.breaks, 2);
        assert_acyclic_after_cuts(&g, &c.nodes, &c.cuts);
    }

    #[test]
    fn two_disjoint_cycles_in_one_scc_need_two_cuts() {
        // 0<->1 and 0<->2: one SCC (shared node 0), two edge-disjoint cycles.
        let (g, _) = graph_with(
            &["a", "b", "c"],
            &[(0, 1, 1), (1, 0, 1), (0, 2, 1), (2, 0, 1)],
        );
        let r = report(&g);
        assert_eq!(r.clusters.len(), 1);
        let c = &r.clusters[0];
        assert_eq!(c.nodes.len(), 3);
        assert_eq!(c.cycles.len(), 2);
        assert_eq!(c.cuts.len(), 2);
        assert_acyclic_after_cuts(&g, &c.nodes, &c.cuts);
    }

    #[test]
    fn independent_sccs_form_separate_clusters() {
        // 0<->1 and 2<->3 are two independent SCCs.
        let (g, _) = graph_with(
            &["a", "b", "c", "d"],
            &[(0, 1, 1), (1, 0, 1), (2, 3, 1), (3, 2, 1)],
        );
        let r = report(&g);
        assert_eq!(r.clusters.len(), 2);
        for c in &r.clusters {
            assert_eq!(c.cuts.len(), 1);
            assert_acyclic_after_cuts(&g, &c.nodes, &c.cuts);
        }
    }

    #[test]
    fn clusters_sorted_by_cut_count_ascending() {
        // SCC {0,1}: 1 cut. SCC {2,3,4}: 2 cuts (0<->1 style pair on node 2).
        let (g, _) = graph_with(
            &["a", "b", "c", "d", "e"],
            &[
                (0, 1, 1),
                (1, 0, 1),
                (2, 3, 1),
                (3, 2, 1),
                (2, 4, 1),
                (4, 2, 1),
            ],
        );
        let r = report(&g);
        assert_eq!(r.clusters.len(), 2);
        assert_eq!(r.clusters[0].cuts.len(), 1);
        assert_eq!(r.clusters[1].cuts.len(), 2);
    }

    #[test]
    fn report_is_deterministic() {
        let deps = &[(0, 1, 3), (1, 2, 1), (2, 0, 2), (1, 3, 1), (3, 0, 4)];
        let (g1, _) = graph_with(&["m0", "m1", "m2", "m3"], deps);
        let (g2, _) = graph_with(&["m0", "m1", "m2", "m3"], deps);
        let a = report(&g1);
        let b = report(&g2);
        assert_eq!(a.clusters.len(), b.clusters.len());
        for (ca, cb) in a.clusters.iter().zip(&b.clusters) {
            let ka: Vec<_> = ca
                .cuts
                .iter()
                .map(|c| (c.from, c.to, c.breaks, c.refs))
                .collect();
            let kb: Vec<_> = cb
                .cuts
                .iter()
                .map(|c| (c.from, c.to, c.breaks, c.refs))
                .collect();
            assert_eq!(ka, kb);
        }
    }
}
