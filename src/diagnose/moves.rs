//! Moves: what a cluster becomes when one of its items gets a module of its
//! own.
//!
//! A move puts one item into a new module, with the `impl` blocks that count
//! for it ([`crate::model::SymbolUse::owner`]). The references to the item
//! then lead to the new module, and the new module makes the references the
//! item made, those to items of its old module included. Every other
//! reference stays where it is. An edge runs between two modules after the
//! move where the references between them still carry one, by the rule of
//! [`ArcGraph::production_subgraph`].

use crate::graph::{ArcGraph, Reexports, Reference};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};

/// One move that leaves only clusters smaller than the cluster it is tried
/// in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Move {
    /// The item that moves.
    pub symbol: String,
    /// The qualified node name of the module that defines it.
    pub provider: String,
    /// The cluster `provider` lies in, as an index into
    /// [`MoveReport::clusters`].
    pub cluster: usize,
    /// The sizes of the clusters that the modules of `cluster` and the new
    /// module form after the move, largest first.
    pub sizes_after: Vec<usize>,
    /// The modules of `cluster` that lie in no cluster after the move, by
    /// their qualified node names in name order.
    pub released: Vec<String>,
}

impl Move {
    /// The size of the largest cluster the move leaves, 0 when it leaves
    /// none.
    fn largest_after(&self) -> usize {
        self.sizes_after.first().copied().unwrap_or(0)
    }

    /// The number of modules the move leaves in clusters.
    fn modules_in_clusters_after(&self) -> usize {
        self.sizes_after.iter().sum()
    }
}

/// The moves that shrink the clusters of a graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MoveReport {
    /// Each cluster by the qualified node names of its modules in name
    /// order: the largest cluster first, clusters of one size by their
    /// first module.
    pub clusters: Vec<Vec<String>>,
    /// The number of moves tried: one per item that makes or receives a
    /// reference of production code between two modules of a cluster.
    pub candidates: usize,
    /// In the order of `clusters`, then the move that leaves the smallest
    /// largest cluster first, then the one that leaves the fewest modules in
    /// clusters, then by provider and symbol.
    pub moves: Vec<Move>,
}

impl ArcGraph {
    /// Try every move in the clusters of the production subgraph under
    /// `reexports` and return those that shrink them. `references` are the
    /// references behind the module edges of this graph.
    #[must_use]
    pub(crate) fn moves(&self, reexports: Reexports, references: &[Reference]) -> MoveReport {
        let sub = self.production_subgraph(reexports);
        let mut clusters: Vec<Vec<(String, NodeIndex)>> = tarjan_scc(&sub)
            .into_iter()
            .filter(|component| component.len() > 1)
            .map(|component| {
                let mut modules: Vec<(String, NodeIndex)> = component
                    .into_iter()
                    .map(|node| (self.qualified_name(sub[node]), node))
                    .collect();
                modules.sort();
                modules
            })
            .collect();
        clusters.sort_by(|a, b| (Reverse(a.len()), &a[0].0).cmp(&(Reverse(b.len()), &b[0].0)));

        let mut report = MoveReport {
            clusters: Vec::new(),
            candidates: 0,
            moves: Vec::new(),
        };
        for (index, modules) in clusters.into_iter().enumerate() {
            let cluster = ClusterReferences::new(index, reexports, &sub, modules, references);
            let candidates = cluster.candidates();
            report.candidates += candidates.len();
            let mut moves: Vec<Move> = candidates
                .into_iter()
                .filter_map(|(provider, item)| cluster.shrinking_move(provider, item))
                .collect();
            moves
                .sort_by_key(|a_move| (a_move.largest_after(), a_move.modules_in_clusters_after()));
            report.moves.extend(moves);
            report.clusters.push(cluster.modules);
        }
        report
    }
}

/// One cluster with the references between and inside its modules. A module
/// is named by its position in `modules`.
struct ClusterReferences<'a> {
    /// Its index in [`MoveReport::clusters`].
    index: usize,
    /// Which re-exports carry an edge of the subgraph it is a cluster of.
    reexports: Reexports,
    /// The qualified node names of its modules, in name order.
    modules: Vec<String>,
    /// Its edges, each with the references behind it.
    edges: Vec<((usize, usize), Vec<&'a Reference>)>,
    /// Per module, its references to its own items.
    own: Vec<Vec<&'a Reference>>,
}

impl<'a> ClusterReferences<'a> {
    /// Gather, for the cluster of `modules` in `sub`, the production subgraph
    /// under `reexports`, its edges and those of `references` that lie in it.
    fn new(
        index: usize,
        reexports: Reexports,
        sub: &DiGraph<NodeIndex, ()>,
        modules: Vec<(String, NodeIndex)>,
        references: &'a [Reference],
    ) -> Self {
        let position_of_node: HashMap<NodeIndex, usize> = modules
            .iter()
            .enumerate()
            .map(|(position, (_, node))| (*node, position))
            .collect();
        let position_of_name: HashMap<&str, usize> = modules
            .iter()
            .enumerate()
            .map(|(position, (name, _))| (name.as_str(), position))
            .collect();

        let mut between: HashMap<(usize, usize), Vec<&Reference>> = HashMap::new();
        let mut own = vec![Vec::new(); modules.len()];
        for reference in references {
            let ends = (
                position_of_name.get(reference.module.as_str()),
                position_of_name.get(reference.provider.as_str()),
            );
            match ends {
                (Some(&from), Some(&to)) if from == to => own[from].push(reference),
                (Some(&from), Some(&to)) => between.entry((from, to)).or_default().push(reference),
                _ => {}
            }
        }
        let mut edges: Vec<((usize, usize), Vec<&Reference>)> = modules
            .iter()
            .flat_map(|(_, node)| sub.edges(*node))
            .filter_map(|edge| {
                let ends = (
                    position_of_node[&edge.source()],
                    *position_of_node.get(&edge.target())?,
                );
                Some((ends, between.remove(&ends).unwrap_or_default()))
            })
            .collect();
        edges.sort_by_key(|(ends, _)| *ends);

        Self {
            index,
            reexports,
            modules: modules.into_iter().map(|(name, _)| name).collect(),
            edges,
            own,
        }
    }

    /// The items a move is tried for, each with its module: the item that
    /// makes and the item that receives a reference of production code
    /// behind an edge.
    fn candidates(&self) -> BTreeSet<(usize, &'a str)> {
        let mut candidates = BTreeSet::new();
        for ((from, to), references) in &self.edges {
            for reference in references.iter().filter(|reference| reference.production) {
                if let Some(user) = &reference.user {
                    candidates.insert((*from, user.as_str()));
                }
                if let Some(item) = &reference.item {
                    candidates.insert((*to, item.as_str()));
                }
            }
        }
        candidates
    }

    /// The move of `item` out of `provider`, when every cluster it leaves is
    /// smaller than this one.
    fn shrinking_move(&self, provider: usize, item: &str) -> Option<Move> {
        let new_module = self.modules.len();
        let components = tarjan_scc(&self.after_move(provider, item));
        let mut sizes_after: Vec<usize> = components
            .iter()
            .map(Vec::len)
            .filter(|size| *size > 1)
            .collect();
        if sizes_after.iter().any(|size| *size >= self.modules.len()) {
            return None;
        }
        sizes_after.sort_unstable_by_key(|size| Reverse(*size));
        let mut released: Vec<String> = components
            .iter()
            .filter(|component| component.len() == 1)
            .map(|component| component[0].index())
            .filter(|module| *module != new_module)
            .map(|module| self.modules[module].clone())
            .collect();
        released.sort();
        Some(Move {
            symbol: item.to_string(),
            provider: self.modules[provider].clone(),
            cluster: self.index,
            sizes_after,
            released,
        })
    }

    /// The cluster's graph after `item` moved out of `provider`: a node per
    /// module by its position, and one more for the new module. An edge that
    /// no reference explains stays at `provider`.
    fn after_move(&self, provider: usize, item: &str) -> DiGraph<(), ()> {
        let new_module = self.modules.len();
        let mut graph = DiGraph::new();
        for _ in 0..=new_module {
            graph.add_node(());
        }
        let mut connect = |from: usize, to: usize| {
            graph.add_edge(NodeIndex::new(from), NodeIndex::new(to), ());
        };
        let makes = |reference: &Reference| reference.user.as_deref() == Some(item);
        let receives = |reference: &Reference| reference.item.as_deref() == Some(item);

        for ((from, to), references) in &self.edges {
            let (from, to) = (*from, *to);
            if from != provider && to != provider {
                connect(from, to);
                continue;
            }
            let (moving, staying): (Vec<&Reference>, Vec<&Reference>) =
                references.iter().partition(|reference| {
                    if from == provider {
                        makes(reference)
                    } else {
                        receives(reference)
                    }
                });
            if self.carries_edge(&staying) || !self.carries_edge(references) {
                connect(from, to);
            }
            if self.carries_edge(&moving) {
                if from == provider {
                    connect(new_module, to);
                } else {
                    connect(from, new_module);
                }
            }
        }

        let (by_the_item, by_the_rest): (Vec<&Reference>, Vec<&Reference>) = self.own[provider]
            .iter()
            .partition(|reference| makes(reference));
        let to_the_rest: Vec<&Reference> = by_the_item
            .into_iter()
            .filter(|reference| !receives(reference))
            .collect();
        let to_the_item: Vec<&Reference> = by_the_rest
            .into_iter()
            .filter(|reference| receives(reference))
            .collect();
        if self.carries_edge(&to_the_rest) {
            connect(new_module, provider);
        }
        if self.carries_edge(&to_the_item) {
            connect(provider, new_module);
        }
        graph
    }

    /// Whether `references` from one module to another carry an edge of
    /// [`ArcGraph::production_subgraph`]: production code makes one of them,
    /// and one of them is no re-export unless those are included.
    fn carries_edge(&self, references: &[&Reference]) -> bool {
        references.iter().any(|reference| reference.production)
            && (self.reexports == Reexports::Included
                || references.iter().any(|reference| !reference.reexport))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DependencyRef, EdgeContext, ModuleInfo, ModuleTree, TargetRoot, TestKind};
    use crate::test_support::conventional_package;

    fn reference(
        module: &str,
        user: Option<&str>,
        provider: &str,
        item: Option<&str>,
    ) -> Reference {
        Reference {
            module: module.to_string(),
            provider: provider.to_string(),
            user: user.map(str::to_string),
            item: item.map(str::to_string),
            production: true,
            reexport: false,
        }
    }

    /// `a` and `c` use each other through `g`, and `a` and `b` use each
    /// other through an edge from `a` that no reference stands behind. With
    /// `g` moved out, that edge keeps `a` and `b` in a cluster, beside the
    /// one of `c` and the new module.
    #[test]
    fn an_edge_without_a_reference_stays_at_its_module() {
        let references = [
            reference("b", None, "a", Some("h")),
            reference("a", Some("g"), "c", Some("x")),
            reference("c", Some("y"), "a", Some("g")),
        ];
        let (a, b, c) = (0, 1, 2);
        let cluster = ClusterReferences {
            index: 0,
            reexports: Reexports::Excluded,
            modules: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            edges: vec![
                ((a, b), vec![]),
                ((a, c), vec![&references[1]]),
                ((b, a), vec![&references[0]]),
                ((c, a), vec![&references[2]]),
            ],
            own: vec![Vec::new(); 3],
        };

        let after = cluster.shrinking_move(a, "g").unwrap();

        assert_eq!(after.sizes_after, [2, 2]);
        assert_eq!(after.released, Vec::<String>::new());
    }

    fn module(name: &str, dependencies: Vec<DependencyRef>) -> ModuleInfo {
        ModuleInfo {
            name: name.to_string(),
            full_path: format!("crate::{name}"),
            file: None,
            children: Vec::new(),
            dependencies,
            own_item_uses: Vec::new(),
            definitions: HashMap::new(),
        }
    }

    /// A reference to `base` in the module `core`.
    fn to_core(line: usize, context: EdgeContext, via_reexport: bool) -> DependencyRef {
        DependencyRef {
            target_package: "crate".to_string(),
            target_module: "core".to_string(),
            target_item: Some("base".to_string()),
            source_file: "src/user.rs".into(),
            line,
            context,
            via_reexport,
            uses: Vec::new(),
            target_root: TargetRoot::AsSeen,
        }
    }

    /// Each module reaches `core` through another mix of references. The
    /// production subgraph and `carries_edge` read the same references
    /// through different types, and a move is predicted right only while
    /// both give the same edges.
    #[test]
    fn references_carry_an_edge_where_the_production_subgraph_has_one() {
        let production = EdgeContext::production;
        let test = || EdgeContext::test(TestKind::Unit);
        let packages = vec![conventional_package("app", "/path/to/app")];
        let modules = vec![ModuleTree {
            root: ModuleInfo {
                full_path: "crate".to_string(),
                children: vec![
                    module("core", Vec::new()),
                    module("in_code", vec![to_core(1, production(), false)]),
                    module("in_test", vec![to_core(1, test(), false)]),
                    module("reexported", vec![to_core(1, production(), true)]),
                    module(
                        "reexported_and_used",
                        vec![
                            to_core(1, production(), true),
                            to_core(2, production(), false),
                        ],
                    ),
                    module(
                        "reexported_and_tested",
                        vec![to_core(1, production(), true), to_core(2, test(), false)],
                    ),
                ],
                ..module("app", Vec::new())
            },
            binaries: Vec::new(),
            unparsed: Vec::new(),
        }];
        let graph = ArcGraph::build(&packages, &modules, None, false);
        let references = crate::graph::references(&packages, &modules);

        let expected = [
            ("in_code", true, true),
            ("in_test", false, false),
            ("reexported", false, true),
            ("reexported_and_used", true, true),
            ("reexported_and_tested", true, true),
        ];
        for (user, excluded, included) in expected {
            for (reexports, edge) in [
                (Reexports::Excluded, excluded),
                (Reexports::Included, included),
            ] {
                let user_name = format!("app::{user}");
                let sub = graph.production_subgraph(reexports);
                let in_subgraph = sub.edge_references().any(|found| {
                    graph.qualified_name(sub[found.source()]) == user_name
                        && graph.qualified_name(sub[found.target()]) == "app::core"
                });
                let cluster = ClusterReferences {
                    index: 0,
                    reexports,
                    modules: Vec::new(),
                    edges: Vec::new(),
                    own: Vec::new(),
                };
                let behind: Vec<&Reference> = references
                    .iter()
                    .filter(|reference| {
                        reference.module == user_name && reference.provider == "app::core"
                    })
                    .collect();

                assert_eq!(
                    (in_subgraph, cluster.carries_edge(&behind)),
                    (edge, edge),
                    "{user} under {reexports:?}"
                );
            }
        }
    }
}
