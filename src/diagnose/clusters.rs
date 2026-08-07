//! Cluster-level cycle aggregation with verified feedback edge sets.
//!
//! Aggregates the representative cycles ([`CycleAnalysis`]) into
//! strongly-connected clusters and, per cluster, computes a feedback arc set
//! (an edge set whose removal leaves no reported cycle standing): greedy
//! set-cover over the representative cycles, then a single `tarjan_scc`
//! verification with a residual re-cover loop.
//!
//! The caller names the cycles it tolerates and the set breaks the rest, so
//! tolerating nothing makes the cluster acyclic.

use super::cycles::{Cycle, CycleAnalysis, RepresentativeCycles};
use crate::graph::{ArcGraph, Edge};
use crate::model::SourceLocation;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

/// One SCC-internal edge, measured against the cluster's cycles.
pub struct CyclicEdge {
    pub from: NodeIndex,
    pub to: NodeIndex,
    /// Gross cycles through this edge within the cluster (order-independent).
    pub cycles: usize,
    /// Distinct symbols crossing the edge (secondary rank key).
    pub symbols: usize,
}

/// One strongly-connected cluster of mutually cyclic modules.
pub struct Cluster {
    /// Owning crate node (module cycles are always intra-crate).
    pub crate_idx: NodeIndex,
    /// All SCC member modules.
    pub nodes: Vec<NodeIndex>,
    /// Indices into [`CycleAnalysis::cycles`] contained in this cluster.
    pub cycles: Vec<usize>,
    /// Edge set whose removal breaks every cycle the caller does not tolerate,
    /// ranked traffic desc, symbols asc, name asc.
    pub feedback_edges: Vec<CyclicEdge>,
    /// Every SCC-internal edge, ranked like `feedback_edges`.
    pub edges: Vec<CyclicEdge>,
}

/// Report over all cyclic clusters, ordered by feedback edge count ascending.
pub struct ClusterReport {
    pub clusters: Vec<Cluster>,
}

/// How willing the tie-break is to put an edge in the feedback set, given the
/// direction it runs through the module tree. Ordered worst-to-best candidate.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum RemovalBias {
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
    /// Aggregate `analysis` into SCC clusters, each with a feedback set covering
    /// the cycles `analysis` still carries.
    ///
    /// `sub` and `analysis` must come from the same subgraph (`analysis` from
    /// `sub.representative_cycles()`) so that `NodeIndex` values line up and the
    /// feedback sets are computed over the same subgraph.
    ///
    /// `tolerated` names the cycles a caller has already accepted and dropped
    /// from `analysis` while leaving them in `sub`. Such a cycle must not pull
    /// an edge into a feedback set: that edge would carry none of the reported
    /// cycles and read as work on something the caller has decided to keep.
    /// Callers with nothing to tolerate pass `|_| false`.
    #[must_use]
    pub fn cluster_report(
        &self,
        sub: &DiGraph<NodeIndex, ()>,
        analysis: &CycleAnalysis,
        tolerated: impl Fn(&Cycle) -> bool,
    ) -> ClusterReport {
        // Map each node of a non-trivial SCC to its cluster id.
        let mut node_scc: HashMap<NodeIndex, usize> = HashMap::new();
        let mut scc_members: Vec<Vec<NodeIndex>> = Vec::new();
        for comp in tarjan_scc(sub) {
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
            if let Some(&id) = node_scc.get(&cycle.nodes[0]) {
                grouped[id].push(cycle_idx);
            }
        }

        let mut clusters: Vec<Cluster> = Vec::new();
        for (id, nodes) in scc_members.into_iter().enumerate() {
            let cycles = std::mem::take(&mut grouped[id]);
            if cycles.is_empty() {
                continue;
            }
            let node_set: HashSet<NodeIndex> = nodes.iter().copied().collect();
            let in_cluster: HashSet<usize> = cycles.iter().copied().collect();
            let edge_cycles = cluster_edge_cycles(analysis, &in_cluster);
            let feedback_edges =
                self.feedback_edges(sub, &node_set, &edge_cycles, in_cluster, &tolerated);
            let edges = self.cluster_edges(sub, &node_set, &edge_cycles);
            let crate_idx = self.owning_crate(nodes[0]);
            clusters.push(Cluster {
                crate_idx,
                nodes,
                cycles,
                feedback_edges,
                edges,
            });
        }

        // Default ordering: smallest feedback set first, then deterministic
        // tiebreaks.
        clusters.sort_by_key(cluster_sort_key);
        ClusterReport { clusters }
    }

    /// Greedy set-cover feedback arc set for one cluster, ranked best-first.
    ///
    /// The cover breaks every cycle in `open`; the `tarjan_scc` pass then
    /// re-covers whatever the residual enumeration still finds untolerated. With
    /// nothing tolerated that pass runs until the cluster is acyclic. With
    /// something tolerated it stops at the enumeration's reach, and
    /// `representative_cycles` does not promise that is every cycle.
    fn feedback_edges(
        &self,
        sub: &DiGraph<NodeIndex, ()>,
        node_set: &HashSet<NodeIndex>,
        edge_cycles: &HashMap<(NodeIndex, NodeIndex), HashSet<usize>>,
        mut open: HashSet<usize>,
        tolerated: impl Fn(&Cycle) -> bool,
    ) -> Vec<CyclicEdge> {
        let mut chosen: Vec<(NodeIndex, NodeIndex)> = Vec::new();
        self.greedy_cover(edge_cycles, &mut open, &mut chosen);

        // The representative-cycle cover alone does not guarantee acyclicity:
        // `representative_cycles` lists a shortest cycle per edge, not every cycle. So
        // keep a `tarjan_scc` guard and re-cover what it finds, minus the cycles
        // `tolerated` accepts: those are still in `sub`, and breaking them is not
        // this set's job.
        let mut removed: HashSet<(NodeIndex, NodeIndex)> = chosen.iter().copied().collect();
        loop {
            let pruned = restricted_subgraph(sub, node_set, &removed);
            if tarjan_scc(&pruned).iter().all(|scc| scc.len() <= 1) {
                break;
            }
            let mut residual = pruned.representative_cycles();
            residual.retain_cycles(|cycle| !tolerated(cycle));
            if residual.cycles.is_empty() {
                break;
            }
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

        let mut feedback: Vec<CyclicEdge> = chosen
            .into_iter()
            .map(|(from, to)| self.cycle_edge(from, to, edge_cycles))
            .collect();
        self.rank(&mut feedback);
        feedback
    }

    /// Every SCC-internal edge of the cluster, ranked like the feedback set,
    /// which stays a subset of it.
    fn cluster_edges(
        &self,
        sub: &DiGraph<NodeIndex, ()>,
        node_set: &HashSet<NodeIndex>,
        edge_cycles: &HashMap<(NodeIndex, NodeIndex), HashSet<usize>>,
    ) -> Vec<CyclicEdge> {
        let mut edges: Vec<CyclicEdge> = sub
            .edge_references()
            .filter(|e| node_set.contains(&sub[e.source()]) && node_set.contains(&sub[e.target()]))
            .map(|e| self.cycle_edge(sub[e.source()], sub[e.target()], edge_cycles))
            .collect();
        self.rank(&mut edges);
        edges
    }

    /// One edge measured against the cluster's cycles: an edge outside
    /// `edge_cycles` carries none of them.
    fn cycle_edge(
        &self,
        from: NodeIndex,
        to: NodeIndex,
        edge_cycles: &HashMap<(NodeIndex, NodeIndex), HashSet<usize>>,
    ) -> CyclicEdge {
        CyclicEdge {
            from,
            to,
            cycles: edge_cycles.get(&(from, to)).map_or(0, HashSet::len),
            symbols: self.edge_symbols(from, to),
        }
    }

    /// Rank an edge list: traffic desc, symbols asc, name asc.
    fn rank(&self, edges: &mut [CyclicEdge]) {
        edges.sort_by(|a, b| {
            Reverse(a.cycles)
                .cmp(&Reverse(b.cycles))
                .then(a.symbols.cmp(&b.symbols))
                .then_with(|| {
                    self.qualified_name(a.from)
                        .cmp(&self.qualified_name(b.from))
                })
                .then_with(|| self.qualified_name(a.to).cmp(&self.qualified_name(b.to)))
        });
    }

    /// Repeatedly remove the edge covering the most still-open cycles until none
    /// remain, appending each pick to `chosen`. Ties break on [`RemovalBias`],
    /// then fewer symbols, then smaller name, so the choice is deterministic.
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
                        // module-tree direction outranks the symbol count
                        .then_with(|| self.removal_bias(**ea).cmp(&self.removal_bias(**eb)))
                        // fewer symbols is better, so the lower count ranks greater
                        .then_with(|| {
                            self.edge_symbols(eb.0, eb.1)
                                .cmp(&self.edge_symbols(ea.0, ea.1))
                        })
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

    /// Distinct symbols crossing the module-dependency edge `from -> to`.
    ///
    /// Counting locations instead would read an import group or a glob as a
    /// single reference, no matter how many symbols it carries: one line is one
    /// location. Unnamed references (an unresolved alias, say) share one slot
    /// rather than counting zero, so an edge the resolver cannot name never
    /// reads as free.
    fn edge_symbols(&self, from: NodeIndex, to: NodeIndex) -> usize {
        self.find_edge(from, to)
            .and_then(|e| match &self[e] {
                Edge::ModuleDep { locations, .. } => Some(count_symbols(locations)),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn name_key(&self, edge: (NodeIndex, NodeIndex)) -> (String, String) {
        (self.qualified_name(edge.0), self.qualified_name(edge.1))
    }

    /// Which way the edge runs through the module tree, as the tie-break sees
    /// it.
    ///
    /// A child→parent edge that is itself a pure re-export (the prelude
    /// pattern, `pub use super::*;`) ranks
    /// [`Structural`](RemovalBias::Structural) rather than
    /// [`Preferred`](RemovalBias::Preferred): under the default graph this edge
    /// doesn't exist at all (ADR-022 drops it as non-coupling), so it only
    /// shows up as a candidate under `--include-reexports`, where it's still
    /// just a facade, not a layer to break.
    fn removal_bias(&self, edge: (NodeIndex, NodeIndex)) -> RemovalBias {
        let (from, to) = edge;
        if self.contains_child(from, to) {
            return RemovalBias::Structural;
        }
        if self.contains_child(to, from) {
            return if self
                .find_edge(from, to)
                .is_some_and(|e| self[e].is_reexport_module_dep())
            {
                RemovalBias::Structural
            } else {
                RemovalBias::Preferred
            };
        }
        RemovalBias::Neutral
    }
}

/// Distinct symbols in `locations`, with all unnamed references sharing one
/// slot. Mirrors the usage grouping the SVG carries.
fn count_symbols(locations: &[SourceLocation]) -> usize {
    let named: HashSet<&str> = locations
        .iter()
        .flat_map(|l| l.symbols.iter().map(String::as_str))
        .collect();
    let unnamed = usize::from(locations.iter().any(|l| l.symbols.is_empty()));
    named.len() + unnamed
}

fn cluster_sort_key(c: &Cluster) -> (usize, usize, usize, usize) {
    let min_node = c.nodes.iter().map(|n| n.index()).min().unwrap_or(0);
    (
        c.feedback_edges.len(),
        c.cycles.len(),
        c.nodes.len(),
        min_node,
    )
}

/// Edge -> the cycles of one cluster running through it, named by `in_cluster`.
/// Edges carrying none of them stay out.
fn cluster_edge_cycles(
    analysis: &CycleAnalysis,
    in_cluster: &HashSet<usize>,
) -> HashMap<(NodeIndex, NodeIndex), HashSet<usize>> {
    analysis
        .edge_cycles
        .iter()
        .filter_map(|(&edge, cyc_list)| {
            let here: HashSet<usize> = cyc_list
                .iter()
                .copied()
                .filter(|c| in_cluster.contains(c))
                .collect();
            (!here.is_empty()).then_some((edge, here))
        })
        .collect()
}

/// Copy of `sub` restricted to `node_set`, with `removed` edges dropped. Node
/// weights stay the original `NodeIndex` values so `representative_cycles` on the
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
    use crate::graph::{ArcGraph, Edge, Node, Reexports};
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
        for &(from, to, symbols) in deps {
            // One symbol per line, so the edge reads the same whether the count
            // takes sites or symbols.
            let locations = (0..symbols)
                .map(|i| SourceLocation {
                    file: format!("src/{}.rs", modules[from]).into(),
                    line: i + 1,
                    symbols: vec![format!("Sym{i}")],
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
        report_tolerating(g, |_| false)
    }

    /// Cluster report with the tolerated cycles dropped from the analysis but
    /// left in the subgraph — the shape a baseline run hands in.
    fn report_tolerating(g: &ArcGraph, tolerated: impl Fn(&Cycle) -> bool) -> ClusterReport {
        let sub = g.production_subgraph(Reexports::Included);
        let mut analysis = sub.representative_cycles();
        analysis.retain_cycles(|cycle| !tolerated(cycle));
        g.cluster_report(&sub, &analysis, tolerated)
    }

    /// Assert that removing `feedback` leaves the cluster `nodes` with no cycle
    /// beyond those `tolerated` accepts. With a predicate that tolerates
    /// nothing this is the plain acyclicity check.
    fn assert_only_tolerated_cycles_remain(
        g: &ArcGraph,
        nodes: &[NodeIndex],
        feedback: &[CyclicEdge],
        tolerated: impl Fn(&Cycle) -> bool,
    ) {
        let sub = g.production_subgraph(Reexports::Included);
        let node_set: HashSet<NodeIndex> = nodes.iter().copied().collect();
        let removed: HashSet<(NodeIndex, NodeIndex)> =
            feedback.iter().map(|e| (e.from, e.to)).collect();
        let left = restricted_subgraph(&sub, &node_set, &removed).representative_cycles();
        let unbroken: Vec<&Cycle> = left.cycles.iter().filter(|c| !tolerated(c)).collect();
        assert!(
            unbroken.is_empty(),
            "feedback set left {} untolerated cycle(s) standing: {unbroken:?}",
            unbroken.len()
        );
    }

    /// Locations in `file`, one per line: `symbols_per_line[i]` lists the
    /// symbols imported on line `i + 1`.
    fn locations(file: &str, symbols_per_line: &[&[&str]]) -> Vec<SourceLocation> {
        symbols_per_line
            .iter()
            .enumerate()
            .map(|(i, symbols)| SourceLocation {
                file: file.into(),
                line: i + 1,
                symbols: symbols.iter().map(|s| (*s).to_owned()).collect(),
                module_path: String::new(),
                via_reexport: false,
            })
            .collect()
    }

    #[test]
    fn symbol_count_ignores_the_import_sites_carrying_the_symbols() {
        // Siblings, so both edges rank `RemovalBias::Neutral` and the symbol
        // count alone decides which one lands in the feedback set.
        // `grouped -> split` imports three symbols in one `use` group: one
        // line, one site. `split -> grouped` imports two, one per line.
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let grouped = graph.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let split = graph.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        graph.add_edge(crate_idx, grouped, Edge::Contains);
        graph.add_edge(crate_idx, split, Edge::Contains);
        graph.add_edge(
            grouped,
            split,
            Edge::ModuleDep {
                locations: locations("src/a.rs", &[&["One", "Two", "Three"]]),
                context: EdgeContext::production(),
            },
        );
        graph.add_edge(
            split,
            grouped,
            Edge::ModuleDep {
                locations: locations("src/b.rs", &[&["Four"], &["Five"]]),
                context: EdgeContext::production(),
            },
        );

        let cluster_report = report(&graph);
        let cluster = &cluster_report.clusters[0];
        assert_eq!(cluster.feedback_edges.len(), 1);
        // The import group counts all three symbols, so the two-symbol edge is
        // the cheaper one and gets picked.
        assert_eq!(cluster.feedback_edges[0].symbols, 2);
        assert_eq!(
            (cluster.feedback_edges[0].from, cluster.feedback_edges[0].to),
            (split, grouped)
        );
    }

    #[test]
    fn unnamed_references_share_one_symbol_slot() {
        // An edge the resolver could not name must not read as free: two bare
        // locations count 1, not 0.
        let (mut g, idx) = graph_with(&["a", "b"], &[(0, 1, 2), (1, 0, 1)]);
        let edge = g.find_edge(idx[0], idx[1]).unwrap();
        let Edge::ModuleDep { locations, .. } = &mut g[edge] else {
            unreachable!("graph_with builds module deps")
        };
        for loc in locations.iter_mut() {
            loc.symbols.clear();
        }
        assert_eq!(g.edge_symbols(idx[0], idx[1]), 1);
    }

    #[test]
    fn acyclic_graph_yields_no_clusters() {
        let (g, _) = graph_with(&["a", "b", "c"], &[(0, 1, 1), (1, 2, 1)]);
        assert!(report(&g).clusters.is_empty());
    }

    #[test]
    fn child_to_parent_edge_is_preferred_even_with_more_symbols() {
        // `parent` is the parent module, `child` is nested inside it.
        // `parent -> child` is a re-export (`pub use child::X`, 2 symbols),
        // `child -> parent` a plain import (`use super::Y`, 5 symbols). The
        // removal bias outranks the symbol count, so `child -> parent` is
        // picked despite carrying more.
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let parent = graph.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let child = graph.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        graph.add_edge(crate_idx, parent, Edge::Contains);
        graph.add_edge(parent, child, Edge::Contains);

        let reexport_locations = (0..2)
            .map(|i| SourceLocation {
                file: "src/a.rs".into(),
                line: i + 1,
                symbols: vec![format!("Sym{i}")],
                module_path: String::new(),
                via_reexport: true,
            })
            .collect();
        graph.add_edge(
            parent,
            child,
            Edge::ModuleDep {
                locations: reexport_locations,
                context: EdgeContext::production(),
            },
        );

        let plain_locations = (0..5)
            .map(|i| SourceLocation {
                file: "src/b.rs".into(),
                line: i + 1,
                symbols: vec![format!("Sym{i}")],
                module_path: String::new(),
                via_reexport: false,
            })
            .collect();
        graph.add_edge(
            child,
            parent,
            Edge::ModuleDep {
                locations: plain_locations,
                context: EdgeContext::production(),
            },
        );

        let cluster_report = report(&graph);
        assert_eq!(cluster_report.clusters.len(), 1);
        let cluster = &cluster_report.clusters[0];
        assert_eq!(cluster.cycles.len(), 1);
        assert_eq!(cluster.feedback_edges.len(), 1);
        let picked = &cluster.feedback_edges[0];
        assert_eq!((picked.from, picked.to), (child, parent));
        assert_eq!(picked.symbols, 5);
        assert_only_tolerated_cycles_remain(
            &graph,
            &cluster.nodes,
            &cluster.feedback_edges,
            |_| false,
        );
    }

    #[test]
    fn reexport_child_to_parent_edge_is_not_preferred() {
        // `child -> parent` is a re-export (`pub use super::*;`, the prelude
        // pattern), so it ranks `RemovalBias::Structural`, not `Preferred`.
        // The bias never favours it and the pick goes to the cheapest neutral
        // edge, even though the re-export carries the fewest symbols.
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let parent = graph.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let child = graph.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        let unrelated = graph.add_node(Node::Module {
            name: "c".into(),
            crate_idx,
        });
        graph.add_edge(crate_idx, parent, Edge::Contains);
        graph.add_edge(parent, child, Edge::Contains);
        graph.add_edge(crate_idx, unrelated, Edge::Contains);

        let reexport_locations = (0..1)
            .map(|i| SourceLocation {
                file: "src/b.rs".into(),
                line: i + 1,
                symbols: vec![format!("Sym{i}")],
                module_path: String::new(),
                via_reexport: true,
            })
            .collect();
        graph.add_edge(
            child,
            parent,
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
                    symbols: vec![format!("Sym{i}")],
                    module_path: String::new(),
                    via_reexport: false,
                })
                .collect()
        };
        graph.add_edge(
            parent,
            unrelated,
            Edge::ModuleDep {
                locations: make_locations("src/a.rs", 3),
                context: EdgeContext::production(),
            },
        );
        graph.add_edge(
            unrelated,
            child,
            Edge::ModuleDep {
                locations: make_locations("src/c.rs", 5),
                context: EdgeContext::production(),
            },
        );

        let cluster_report = report(&graph);
        assert_eq!(cluster_report.clusters.len(), 1);
        let cluster = &cluster_report.clusters[0];
        assert_eq!(cluster.cycles.len(), 1);
        assert_eq!(cluster.feedback_edges.len(), 1);
        let picked = &cluster.feedback_edges[0];
        assert_eq!((picked.from, picked.to), (parent, unrelated));
        assert_eq!(picked.symbols, 3);
        assert_only_tolerated_cycles_remain(
            &graph,
            &cluster.nodes,
            &cluster.feedback_edges,
            |_| false,
        );
    }

    #[test]
    fn fewer_symbols_wins_without_a_parent_child_relation() {
        // a <-> b, siblings under the crate (no Contains edge between them).
        // With no module-tree prior to apply, the symbol-count tie-break still
        // decides: a->b carries 2 symbols, b->a carries 5.
        let (g, idx) = graph_with(&["a", "b"], &[(0, 1, 2), (1, 0, 5)]);
        let r = report(&g);
        assert_eq!(r.clusters.len(), 1);
        let c = &r.clusters[0];
        assert_eq!(c.cycles.len(), 1);
        assert_eq!(c.feedback_edges.len(), 1);
        let picked = &c.feedback_edges[0];
        assert_eq!((picked.from, picked.to), (idx[0], idx[1]));
        assert_eq!(picked.symbols, 2);
        assert_eq!(picked.cycles, 1);
        assert_only_tolerated_cycles_remain(&g, &c.nodes, &c.feedback_edges, |_| false);
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
        assert_eq!(c.feedback_edges.len(), 1);
        let picked = &c.feedback_edges[0];
        assert_eq!((picked.from, picked.to), (idx[0], idx[1]));
        assert_eq!(picked.cycles, 2);
        assert_only_tolerated_cycles_remain(&g, &c.nodes, &c.feedback_edges, |_| false);
    }

    #[test]
    fn two_disjoint_cycles_in_one_scc_need_two_feedback_edges() {
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
        assert_eq!(c.feedback_edges.len(), 2);
        assert_only_tolerated_cycles_remain(&g, &c.nodes, &c.feedback_edges, |_| false);
    }

    /// Three 2-cycles through one hub module: `hub<->a`, `hub<->b`, `hub<->c`.
    /// One SCC, three edge-disjoint cycles.
    const HUB_MODULES: &[&str] = &["hub", "a", "b", "c"];
    const HUB_DEPS: &[(usize, usize, usize)] = &[
        (0, 1, 1),
        (1, 0, 1),
        (0, 2, 1),
        (2, 0, 1),
        (0, 3, 1),
        (3, 0, 1),
    ];

    #[test]
    fn every_reported_cycle_gets_an_edge_when_nothing_is_tolerated() {
        let (g, idx) = graph_with(HUB_MODULES, HUB_DEPS);
        let r = report(&g);
        assert_eq!(r.clusters.len(), 1);
        let c = &r.clusters[0];
        assert_eq!(c.cycles.len(), 3);
        let picked: Vec<_> = c.feedback_edges.iter().map(|e| (e.from, e.to)).collect();
        assert_eq!(
            picked,
            vec![(idx[1], idx[0]), (idx[2], idx[0]), (idx[3], idx[0])]
        );
        assert_only_tolerated_cycles_remain(&g, &c.nodes, &c.feedback_edges, |_| false);
    }

    #[test]
    fn a_tolerated_cycle_pulls_no_edge_into_the_feedback_set() {
        // hub<->a is tolerated, so it is out of the analysis while its edges
        // stay in the subgraph. Breaking it is not this report's job: the two
        // reported cycles need one edge each, and neither of them may be an
        // edge of the tolerated cycle.
        let (g, idx) = graph_with(HUB_MODULES, HUB_DEPS);
        let tolerated = |cycle: &Cycle| cycle.nodes.len() == 2 && cycle.nodes.contains(&idx[1]);
        let r = report_tolerating(&g, tolerated);

        assert_eq!(r.clusters.len(), 1);
        let c = &r.clusters[0];
        assert_eq!(c.cycles.len(), 2);
        assert_eq!(c.feedback_edges.len(), 2);
        // No edge carrying zero reported cycles: that is what a feedback set
        // reaching past the reported cycles looks like in the output.
        assert!(c.feedback_edges.iter().all(|e| e.cycles > 0));
        let picked: Vec<_> = c.feedback_edges.iter().map(|e| (e.from, e.to)).collect();
        assert_eq!(picked, vec![(idx[2], idx[0]), (idx[3], idx[0])]);
        assert_only_tolerated_cycles_remain(&g, &c.nodes, &c.feedback_edges, tolerated);
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
            assert_eq!(c.feedback_edges.len(), 1);
            assert_only_tolerated_cycles_remain(&g, &c.nodes, &c.feedback_edges, |_| false);
        }
    }

    #[test]
    fn clusters_sorted_by_feedback_edge_count_ascending() {
        // SCC {0,1}: 1 feedback edge. SCC {2,3,4}: 2 (0<->1 style pair on node 2).
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
        assert_eq!(r.clusters[0].feedback_edges.len(), 1);
        assert_eq!(r.clusters[1].feedback_edges.len(), 2);
    }

    #[test]
    fn edges_lists_every_scc_internal_edge_feedback_only_the_chosen_ones() {
        // Two triangles sharing directed edge 0->1: 5 SCC-internal edges total,
        // but only one is needed to break both cycles.
        let (g, _) = graph_with(
            &["m0", "m1", "m2", "m3"],
            &[(0, 1, 1), (1, 2, 1), (2, 0, 1), (1, 3, 1), (3, 0, 1)],
        );
        let r = report(&g);
        let c = &r.clusters[0];
        assert_eq!(c.feedback_edges.len(), 1);
        assert_eq!(c.edges.len(), 5);
    }

    #[test]
    fn edges_are_ranked_like_the_feedback_set_which_stays_a_subset() {
        // a<->b (5, 1 symbols) and a<->c (3, 2 symbols): one SCC of three
        // nodes, two disjoint 2-cycles through a. Every edge participates in
        // exactly one cycle, so symbols decide order across all four edges.
        let (g, idx) = graph_with(
            &["a", "b", "c"],
            &[(0, 1, 5), (1, 0, 1), (0, 2, 3), (2, 0, 2)],
        );
        let r = report(&g);
        let c = &r.clusters[0];

        assert_eq!(c.feedback_edges.len(), 2);
        let feedback: Vec<_> = c.feedback_edges.iter().map(|e| (e.from, e.to)).collect();
        assert_eq!(feedback, vec![(idx[1], idx[0]), (idx[2], idx[0])]);

        assert_eq!(c.edges.len(), 4);
        let edges: Vec<_> = c.edges.iter().map(|e| (e.from, e.to)).collect();
        assert_eq!(
            edges,
            vec![
                (idx[1], idx[0]), // b->a, 1 symbol
                (idx[2], idx[0]), // c->a, 2 symbols
                (idx[0], idx[2]), // a->c, 3 symbols
                (idx[0], idx[1]), // a->b, 5 symbols
            ]
        );
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
                .feedback_edges
                .iter()
                .map(|e| (e.from, e.to, e.cycles, e.symbols))
                .collect();
            let kb: Vec<_> = cb
                .feedback_edges
                .iter()
                .map(|e| (e.from, e.to, e.cycles, e.symbols))
                .collect();
            assert_eq!(ka, kb);
        }
    }
}
