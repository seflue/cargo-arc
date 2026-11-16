//! Levels: how far above the modules it uses each module of a cluster lies.
//!
//! A level is Lakos's, counted inside a package: 1 for a module that uses no
//! module of its package, otherwise one more than the highest level among
//! the modules of its package it uses. No levels satisfy every edge of a
//! cluster, so its modules are first put into a level order, a module before
//! the modules it uses, that leaves few symbols on the edges against it. The
//! levels then leave out the edges against the order that close a cycle.

use crate::graph::{ArcGraph, Reexports};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{DfsPostOrder, EdgeRef};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// A module of a cluster with its level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModuleLevel {
    /// The qualified node name of the module.
    pub module: String,
    pub level: usize,
}

/// The level of each node of the production subgraph, and the edges the
/// levels leave out: the upward edges.
pub(crate) struct Levelization {
    reexports: Reexports,
    levels: HashMap<NodeIndex, usize>,
    upward: HashSet<(NodeIndex, NodeIndex)>,
    ties: HashSet<(NodeIndex, NodeIndex)>,
}

impl Levelization {
    /// Return the `Reexports` the production subgraph was taken under.
    #[must_use]
    pub(crate) fn reexports(&self) -> Reexports {
        self.reexports
    }

    /// Return the level of `node` inside its package.
    #[must_use]
    pub(crate) fn level(&self, node: NodeIndex) -> usize {
        self.levels[&node]
    }

    /// Whether the edge from `user` to `provider` runs against the level
    /// order of its cluster and closes a cycle.
    #[must_use]
    pub(crate) fn is_upward(&self, user: NodeIndex, provider: NodeIndex) -> bool {
        self.upward.contains(&(user, provider))
    }

    /// Whether the edge from `user` to `provider` is upward and the edge
    /// back carries as many symbols, so the two edges alone do not decide
    /// which of them is upward.
    #[must_use]
    pub(crate) fn is_tie(&self, user: NodeIndex, provider: NodeIndex) -> bool {
        self.ties.contains(&(user, provider))
    }
}

/// The number of symbols on each edge between two modules of a cluster. A
/// module is named by its position among the cluster's modules in name
/// order, an edge by the positions of its user and its provider.
type SymbolCounts = BTreeMap<(usize, usize), usize>;

impl ArcGraph {
    /// Return the clusters of the production subgraph under `reexports`,
    /// each as its modules with their levels: the largest cluster first and
    /// clusters of one size by their first module in name order, the
    /// modules of a cluster by level, the highest first, then by name.
    #[must_use]
    pub(crate) fn cluster_levels(&self, reexports: Reexports) -> Vec<Vec<ModuleLevel>> {
        let sub = self.production_subgraph(reexports);
        let clusters = self.clusters_by_size(&sub);
        let levelization = self.levelize(&sub, &clusters, reexports);
        clusters
            .into_iter()
            .map(|cluster| {
                let mut modules: Vec<ModuleLevel> = cluster
                    .into_iter()
                    .map(|(module, node)| ModuleLevel {
                        level: levelization.level(node),
                        module,
                    })
                    .collect();
                modules.sort_by(|a, b| {
                    (Reverse(a.level), &a.module).cmp(&(Reverse(b.level), &b.module))
                });
                modules
            })
            .collect()
    }

    /// Return the levels and the upward edges of the production subgraph
    /// under `reexports`.
    #[must_use]
    pub(crate) fn levelization(&self, reexports: Reexports) -> Levelization {
        let sub = self.production_subgraph(reexports);
        self.levelize(&sub, &self.clusters_by_size(&sub), reexports)
    }

    /// Return the levels of `sub`, the production subgraph under
    /// `reexports`, and the upward edges of its `clusters`. A node of `sub`
    /// has the index of its node in the dependency graph.
    fn levelize(
        &self,
        sub: &DiGraph<NodeIndex, ()>,
        clusters: &[Vec<(String, NodeIndex)>],
        reexports: Reexports,
    ) -> Levelization {
        let mut upward = HashSet::new();
        let mut ties = HashSet::new();
        for cluster in clusters {
            let symbols = self.symbol_counts(sub, cluster);
            let against = edges_against(&level_order(cluster.len(), &symbols), &symbols);
            for (user, provider) in closing_a_cycle(against, &symbols) {
                let edge = (cluster[user].1, cluster[provider].1);
                upward.insert(edge);
                if symbols.get(&(provider, user)) == symbols.get(&(user, provider)) {
                    ties.insert(edge);
                }
            }
        }
        Levelization {
            reexports,
            levels: self.levels(sub, &upward),
            upward,
            ties,
        }
    }

    /// Return the symbol count of each edge between two modules of
    /// `cluster`, a cluster of `sub`. An edge without a symbol counts one.
    fn symbol_counts(
        &self,
        sub: &DiGraph<NodeIndex, ()>,
        cluster: &[(String, NodeIndex)],
    ) -> SymbolCounts {
        let position: HashMap<NodeIndex, usize> = cluster
            .iter()
            .enumerate()
            .map(|(position, (_, node))| (*node, position))
            .collect();
        cluster
            .iter()
            .enumerate()
            .flat_map(|(user, (_, node))| sub.edges(*node).map(move |edge| (user, edge)))
            .filter_map(|(user, edge)| {
                let provider = *position.get(&edge.target())?;
                let count = self
                    .edge_symbols(sub[edge.source()], sub[edge.target()])
                    .len();
                Some(((user, provider), count.max(1)))
            })
            .collect()
    }

    /// Return the level of each node of `sub` inside its package, with the
    /// edges `against` left out: `sub` without them has no cycle.
    fn levels(
        &self,
        sub: &DiGraph<NodeIndex, ()>,
        against: &HashSet<(NodeIndex, NodeIndex)>,
    ) -> HashMap<NodeIndex, usize> {
        // No node is dropped, so both graphs name a node by the same index.
        let inside_packages = sub.filter_map(
            |_, node| Some(*node),
            |edge, ()| {
                let (user, provider) = sub.edge_endpoints(edge)?;
                let same_package =
                    self.owning_package(sub[user]) == self.owning_package(sub[provider]);
                (same_package && !against.contains(&(user, provider))).then_some(())
            },
        );
        let mut levels = HashMap::new();
        let mut walk = DfsPostOrder::empty(&inside_packages);
        for start in inside_packages.node_indices() {
            walk.move_to(start);
            // Without a cycle, a node comes after every node it uses.
            while let Some(node) = walk.next(&inside_packages) {
                let highest_used = inside_packages
                    .neighbors(node)
                    .filter_map(|used| levels.get(&used))
                    .max()
                    .copied()
                    .unwrap_or(0);
                levels.insert(node, highest_used + 1);
            }
        }
        levels
    }
}

/// Put `count` modules into an order, a module before the modules it uses,
/// that leaves few of `symbols` on the edges against it: no move of a single
/// module leaves fewer, though another order can.
fn level_order(count: usize, symbols: &SymbolCounts) -> Vec<usize> {
    let mut used = vec![0; count];
    let mut provided = vec![0; count];
    for (&(user, provider), &weight) in symbols {
        used[user] += weight;
        provided[provider] += weight;
    }
    let mut order: Vec<usize> = (0..count).collect();
    // Start with the modules that use the most symbols beyond those they
    // provide, modules that tie in name order: `a` comes first where
    // used[a] - provided[a] exceeds used[b] - provided[b].
    order.sort_by(|&a, &b| {
        (used[b] + provided[a])
            .cmp(&(used[a] + provided[b]))
            .then(a.cmp(&b))
    });

    loop {
        let mut improved = false;
        for module in order.clone() {
            improved |= move_to_its_best_place(&mut order, module, symbols);
        }
        if !improved {
            return order;
        }
    }
}

/// Move `module` to the first place in `order` that leaves the fewest
/// symbols on the edges against the order, when that is fewer than where it
/// stands. Return whether it moved.
fn move_to_its_best_place(order: &mut Vec<usize>, module: usize, symbols: &SymbolCounts) -> bool {
    let Some(current) = order.iter().position(|&other| other == module) else {
        return false;
    };
    order.remove(current);
    let weight =
        |user: usize, provider: usize| symbols.get(&(user, provider)).copied().unwrap_or(0);

    // Before every other module, `module` is used against the order by each
    // of its users. Each module it passes on the way down turns that
    // module's use of it around, and its own use of that module as well.
    let mut cost: usize = order.iter().map(|&other| weight(other, module)).sum();
    let mut costs = vec![cost];
    for &other in order.iter() {
        cost = cost + weight(module, other) - weight(other, module);
        costs.push(cost);
    }
    let place = (0..costs.len())
        .min_by_key(|&place| costs[place])
        .filter(|&place| costs[place] < costs[current])
        .unwrap_or(current);
    order.insert(place, module);
    place != current
}

/// Return the edges of `symbols` against `order`: those from a module to a
/// module before it.
fn edges_against(order: &[usize], symbols: &SymbolCounts) -> BTreeSet<(usize, usize)> {
    let mut place = vec![0; order.len()];
    for (index, &module) in order.iter().enumerate() {
        place[module] = index;
    }
    symbols
        .keys()
        .copied()
        .filter(|&(user, provider)| place[user] > place[provider])
        .collect()
}

/// Drop from `against` every edge that closes no cycle: one whose provider
/// does not reach its user over the edges of `symbols` outside `against`.
/// A dropped edge counts as one of those edges from then on.
fn closing_a_cycle(
    mut against: BTreeSet<(usize, usize)>,
    symbols: &SymbolCounts,
) -> BTreeSet<(usize, usize)> {
    loop {
        let closes_none = against
            .iter()
            .copied()
            .find(|&(user, provider)| !reaches(provider, user, symbols, &against));
        match closes_none {
            Some(edge) => against.remove(&edge),
            None => return against,
        };
    }
}

/// Whether `from` reaches `to` over the edges of `symbols` outside
/// `against`.
fn reaches(
    from: usize,
    to: usize,
    symbols: &SymbolCounts,
    against: &BTreeSet<(usize, usize)>,
) -> bool {
    let mut seen = BTreeSet::from([from]);
    let mut to_visit = vec![from];
    while let Some(module) = to_visit.pop() {
        if module == to {
            return true;
        }
        let used = symbols
            .range((module, 0)..=(module, usize::MAX))
            .map(|(edge, _)| *edge)
            .filter(|edge| !against.contains(edge));
        for (_, provider) in used {
            if seen.insert(provider) {
                to_visit.push(provider);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::EdgeWeight;
    use crate::model::{EdgeContext, SourceLocation};
    use crate::test_support::{module_node, package_node};
    use std::collections::BTreeSet;

    /// A graph of `modules`, each named `<package>::<module>`, with one
    /// production edge per `(user, provider, symbols)` of `uses`.
    fn graph_of(modules: &[&str], uses: &[(&str, &str, usize)]) -> ArcGraph {
        let mut graph = ArcGraph::new();
        let mut packages: HashMap<&str, NodeIndex> = HashMap::new();
        let mut nodes: HashMap<&str, NodeIndex> = HashMap::new();
        for qualified in modules {
            let (package, module) = qualified.split_once("::").expect("<package>::<module>");
            let package_idx = *packages
                .entry(package)
                .or_insert_with(|| graph.add_node(package_node(package)));
            let node = graph.add_node(module_node(module, package_idx));
            graph.add_edge(package_idx, node, EdgeWeight::Contains);
            nodes.insert(qualified, node);
        }
        for &(user, provider, symbols) in uses {
            let locations = (0..symbols)
                .map(|line| SourceLocation {
                    file: format!("src/{user}.rs").into(),
                    line: line + 1,
                    symbols: vec![format!("Symbol{line}")],
                    module_path: String::new(),
                    via_reexport: false,
                    usage_categories: BTreeSet::new(),
                })
                .collect();
            graph.add_edge(
                nodes[user],
                nodes[provider],
                EdgeWeight::ModuleDep {
                    locations,
                    context: EdgeContext::production(),
                },
            );
        }
        graph
    }

    /// The clusters of `graph` as `(module, level)` pairs.
    fn levels_of(graph: &ArcGraph) -> Vec<Vec<(String, usize)>> {
        graph
            .cluster_levels(Reexports::Excluded)
            .into_iter()
            .map(|cluster| {
                cluster
                    .into_iter()
                    .map(|member| (member.module, member.level))
                    .collect()
            })
            .collect()
    }

    fn pairs(levels: &[(&str, usize)]) -> Vec<(String, usize)> {
        levels
            .iter()
            .map(|(module, level)| ((*module).to_string(), *level))
            .collect()
    }

    /// The modules form one cycle, 0 -> 1 -> 3 -> 2 -> 0, and start in name
    /// order with two edges against it. Module 2 leaves one edge against
    /// the order at the front and at the end, and moves to the front.
    #[test]
    fn level_order_moves_a_module_to_the_first_place_that_leaves_the_fewest_symbols() {
        let symbols = SymbolCounts::from([((0, 1), 1), ((1, 3), 1), ((3, 2), 1), ((2, 0), 1)]);

        assert_eq!(level_order(4, &symbols), [2, 0, 1, 3]);
    }

    #[test]
    fn level_order_puts_modules_that_tie_in_name_order() {
        let symbols = SymbolCounts::from([((0, 1), 1), ((1, 0), 1)]);

        assert_eq!(level_order(2, &symbols), [0, 1]);
    }

    /// Modules 0, 2 and 1 form one cycle, and two of its edges run against
    /// the order. Without the other, neither closes a cycle. Once the first
    /// counts as an ordinary edge, the second closes the cycle.
    #[test]
    fn an_edge_dropped_from_the_edges_against_the_order_can_close_the_cycle_of_another() {
        let symbols = SymbolCounts::from([((0, 2), 1), ((2, 1), 1), ((1, 0), 1)]);
        let against = BTreeSet::from([(1, 0), (2, 1)]);

        assert_eq!(closing_a_cycle(against, &symbols), BTreeSet::from([(2, 1)]));
    }

    /// The modules form one cycle, a -> e -> c -> b -> d -> a. The search
    /// ends at the order b, e, d, c, a, which two edges run against: a -> e
    /// and c -> b. Only c -> b is left out, so a lies above e.
    #[test]
    fn an_edge_against_the_level_order_that_closes_no_cycle_raises_the_level() {
        let graph = graph_of(
            &["app::a", "app::b", "app::c", "app::d", "app::e"],
            &[
                ("app::a", "app::e", 1),
                ("app::e", "app::c", 2),
                ("app::c", "app::b", 1),
                ("app::b", "app::d", 3),
                ("app::d", "app::a", 3),
            ],
        );

        assert_eq!(
            levels_of(&graph),
            [pairs(&[
                ("app::b", 5),
                ("app::d", 4),
                ("app::a", 3),
                ("app::e", 2),
                ("app::c", 1),
            ])]
        );
    }

    #[test]
    fn a_module_of_another_package_raises_no_level() {
        let graph = graph_of(
            &["app::a", "app::b", "base::x", "base::y", "base::z"],
            &[
                ("app::a", "app::b", 2),
                ("app::b", "app::a", 1),
                ("app::b", "base::x", 1),
                ("base::x", "base::y", 1),
                ("base::y", "base::z", 1),
            ],
        );

        assert_eq!(levels_of(&graph), [pairs(&[("app::a", 2), ("app::b", 1)])]);
    }

    /// `a` uses `b` without a symbol and `b` uses one symbol of `a`: the
    /// two edges weigh the same, so the modules stay in name order.
    #[test]
    fn an_edge_without_a_symbol_weighs_as_much_as_one_symbol() {
        let graph = graph_of(
            &["app::a", "app::b"],
            &[("app::a", "app::b", 0), ("app::b", "app::a", 1)],
        );

        assert_eq!(levels_of(&graph), [pairs(&[("app::a", 2), ("app::b", 1)])]);
    }

    fn node(graph: &ArcGraph, module: &str) -> NodeIndex {
        graph
            .node_indices()
            .find(|&node| graph.qualified_name(node) == module)
            .expect("the graph has the module")
    }

    #[test]
    fn the_edge_against_the_level_order_of_a_cycle_is_upward() {
        let graph = graph_of(
            &["app::a", "app::b"],
            &[("app::a", "app::b", 2), ("app::b", "app::a", 1)],
        );
        let (a, b) = (node(&graph, "app::a"), node(&graph, "app::b"));

        let levelization = graph.levelization(Reexports::Excluded);

        assert!(levelization.is_upward(b, a));
        assert!(!levelization.is_upward(a, b));
        assert!(!levelization.is_tie(b, a));
    }

    /// The cycle of the test above: a -> e runs against the level order and
    /// closes no cycle, so only c -> b is upward.
    #[test]
    fn an_edge_against_the_level_order_that_closes_no_cycle_is_not_upward() {
        let graph = graph_of(
            &["app::a", "app::b", "app::c", "app::d", "app::e"],
            &[
                ("app::a", "app::e", 1),
                ("app::e", "app::c", 2),
                ("app::c", "app::b", 1),
                ("app::b", "app::d", 3),
                ("app::d", "app::a", 3),
            ],
        );

        let levelization = graph.levelization(Reexports::Excluded);

        assert!(levelization.is_upward(node(&graph, "app::c"), node(&graph, "app::b")));
        assert!(!levelization.is_upward(node(&graph, "app::a"), node(&graph, "app::e")));
    }

    #[test]
    fn an_upward_edge_is_a_tie_when_the_edge_back_carries_as_many_symbols() {
        let graph = graph_of(
            &["app::a", "app::b"],
            &[("app::a", "app::b", 2), ("app::b", "app::a", 2)],
        );
        let (a, b) = (node(&graph, "app::a"), node(&graph, "app::b"));

        let levelization = graph.levelization(Reexports::Excluded);

        assert!(levelization.is_upward(b, a));
        assert!(levelization.is_tie(b, a));
        assert!(!levelization.is_tie(a, b));
    }

    #[test]
    fn a_module_outside_every_cluster_has_a_level() {
        let graph = graph_of(
            &["app::x", "app::y", "app::z"],
            &[("app::x", "app::y", 1), ("app::y", "app::z", 1)],
        );

        let levelization = graph.levelization(Reexports::Excluded);

        let levels =
            ["app::x", "app::y", "app::z"].map(|module| levelization.level(node(&graph, module)));
        assert_eq!(levels, [3, 2, 1]);
    }
}
