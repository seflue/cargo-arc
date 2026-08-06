//! Importer-set partitioning: which modules import each symbol of a provider.
//!
//! Inverts production `ModuleDep` edges into a per-provider reverse index and
//! groups a provider's symbols by their shared consumer set. Each group carries
//! a [`ConsumerLocality`] describing how close its consumers sit to each other
//! in the module tree.

use crate::graph::{ArcGraph, Edge};
use petgraph::graph::NodeIndex;
use std::collections::{BTreeMap, BTreeSet};

/// Reverse index over all provider modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImporterPartition {
    pub providers: Vec<ProviderPartition>,
}

/// One provider module and its consumer groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPartition {
    pub module: NodeIndex,
    pub consumer_groups: Vec<ConsumerGroup>,
}

/// Symbols of a provider that share the exact same consumer set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerGroup {
    /// Importing modules, deduplicated and sorted by index.
    pub consumers: Vec<NodeIndex>,
    /// Symbols with exactly this consumer set, sorted lexically.
    pub symbols: Vec<String>,
    pub locality: ConsumerLocality<NodeIndex>,
}

/// How close a symbol's consumers sit to each other in the module tree. The
/// node in each variant is the deepest module still enclosing all consumers
/// (their common home).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumerLocality<N> {
    /// Exactly one consumer; the node is that consumer.
    SingleConsumer(N),
    /// All consumers share a common module ancestor (the node), provider outside it.
    CommonAncestor(N),
    /// Spread across the crate, no common module below the crate root.
    CrateWide,
}

impl ArcGraph {
    /// Partition every provider module's symbols by their consumer set.
    #[must_use]
    pub fn importer_partition(&self) -> ImporterPartition {
        // provider -> symbol -> consumers, over production module deps only.
        // A symbol imported via `pub use` is republished, not used, so
        // re-export locations are skipped.
        let mut index: BTreeMap<NodeIndex, BTreeMap<String, BTreeSet<NodeIndex>>> = BTreeMap::new();
        for edge_idx in self.edge_indices() {
            if !self[edge_idx].is_production_module_dep() {
                continue;
            }
            let Edge::ModuleDep { locations, .. } = &self[edge_idx] else {
                continue;
            };
            let Some((consumer, provider)) = self.edge_endpoints(edge_idx) else {
                continue;
            };
            for loc in locations {
                if loc.via_reexport {
                    continue;
                }
                for symbol in &loc.symbols {
                    index
                        .entry(provider)
                        .or_default()
                        .entry(symbol.clone())
                        .or_default()
                        .insert(consumer);
                }
            }
        }

        let providers = index
            .into_iter()
            .map(|(module, symbols)| ProviderPartition {
                module,
                consumer_groups: self.consumer_groups_of(module, symbols),
            })
            .collect();

        ImporterPartition { providers }
    }

    /// Bundle a provider's symbols that share an identical consumer set, one
    /// [`ConsumerGroup`] per set, each tagged with its [`ConsumerLocality`].
    fn consumer_groups_of(
        &self,
        module: NodeIndex,
        symbols: BTreeMap<String, BTreeSet<NodeIndex>>,
    ) -> Vec<ConsumerGroup> {
        let mut groups: BTreeMap<Vec<NodeIndex>, Vec<String>> = BTreeMap::new();
        for (symbol, consumers) in symbols {
            groups
                .entry(consumers.into_iter().collect())
                .or_default()
                .push(symbol);
        }
        groups
            .into_iter()
            .map(|(consumers, mut symbols)| {
                symbols.sort();
                let locality = self.consumer_locality(module, &consumers);
                ConsumerGroup {
                    consumers,
                    symbols,
                    locality,
                }
            })
            .collect()
    }

    /// Classify how close a consumer group's consumers sit in the module tree.
    fn consumer_locality(
        &self,
        provider: NodeIndex,
        consumers: &[NodeIndex],
    ) -> ConsumerLocality<NodeIndex> {
        if let [only] = consumers {
            return ConsumerLocality::SingleConsumer(*only);
        }
        match self.deepest_common_module(consumers) {
            // A common ancestor that already holds the provider is no proper home.
            Some(ancestor) if !self.containment_subtree(ancestor).contains(&provider) => {
                ConsumerLocality::CommonAncestor(ancestor)
            }
            _ => ConsumerLocality::CrateWide,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Node};
    use crate::model::{EdgeContext, SourceLocation};

    /// Flat builder: every module is a direct crate child. Thin wrapper over
    /// [`nested`]. `deps` are `(from, to, symbols)` production `ModuleDep` edges.
    fn graph_with(
        modules: &[&str],
        deps: &[(usize, usize, &[&str])],
    ) -> (ArcGraph, Vec<NodeIndex>) {
        nested(modules, &vec![usize::MAX; modules.len()], deps)
    }

    /// Build a module tree. `parents[i]` is the index (into the returned vec)
    /// of module `i`'s parent module, or `usize::MAX` for a direct crate child;
    /// a parent must be listed before its children. `deps` are
    /// `(from, to, symbols)` production `ModuleDep` edges.
    fn nested(
        names: &[&str],
        parents: &[usize],
        deps: &[(usize, usize, &[&str])],
    ) -> (ArcGraph, Vec<NodeIndex>) {
        let mut g = ArcGraph::new();
        let crate_idx = g.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let mut idx = Vec::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            let n = g.add_node(Node::Module {
                name: (*name).into(),
                crate_idx,
            });
            let parent = if parents[i] == usize::MAX {
                crate_idx
            } else {
                idx[parents[i]]
            };
            g.add_edge(parent, n, Edge::Contains);
            idx.push(n);
        }
        for &(from, to, symbols) in deps {
            let locations = vec![SourceLocation {
                file: format!("src/{}.rs", names[from]).into(),
                line: 1,
                symbols: symbols.iter().map(|s| (*s).to_owned()).collect(),
                module_path: String::new(),
                via_reexport: false,
            }];
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

    #[test]
    fn no_module_deps_yields_no_providers() {
        let (g, _idx) = graph_with(&["model", "user"], &[]);
        assert!(g.importer_partition().providers.is_empty());
    }

    #[test]
    fn single_consumer_locality() {
        // user -> model [Foo]
        let (g, idx) = graph_with(&["model", "user"], &[(1, 0, &["Foo"])]);
        let part = g.importer_partition();
        assert_eq!(part.providers.len(), 1);
        let provider = &part.providers[0];
        assert_eq!(provider.module, idx[0]);
        assert_eq!(
            provider.consumer_groups,
            vec![ConsumerGroup {
                consumers: vec![idx[1]],
                symbols: vec!["Foo".to_owned()],
                locality: ConsumerLocality::SingleConsumer(idx[1]),
            }]
        );
    }

    #[test]
    fn symbols_bundle_by_identical_consumer_set() {
        // model provides Foo,Bar to user and Baz to admin.
        let (g, idx) = graph_with(
            &["model", "user", "admin"],
            &[(1, 0, &["Foo", "Bar"]), (2, 0, &["Baz"])],
        );
        let provider = &g.importer_partition().providers[0];
        assert_eq!(
            provider.consumer_groups,
            vec![
                ConsumerGroup {
                    consumers: vec![idx[1]],
                    symbols: vec!["Bar".to_owned(), "Foo".to_owned()],
                    locality: ConsumerLocality::SingleConsumer(idx[1]),
                },
                ConsumerGroup {
                    consumers: vec![idx[2]],
                    symbols: vec!["Baz".to_owned()],
                    locality: ConsumerLocality::SingleConsumer(idx[2]),
                },
            ]
        );
    }

    #[test]
    fn reexport_locations_are_ignored() {
        let mut g = ArcGraph::new();
        let crate_idx = g.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let model = g.add_node(Node::Module {
            name: "model".into(),
            crate_idx,
        });
        let user = g.add_node(Node::Module {
            name: "user".into(),
            crate_idx,
        });
        g.add_edge(crate_idx, model, Edge::Contains);
        g.add_edge(crate_idx, user, Edge::Contains);
        g.add_edge(
            user,
            model,
            Edge::ModuleDep {
                locations: vec![SourceLocation {
                    file: "src/user.rs".into(),
                    line: 1,
                    symbols: vec!["Foo".to_owned()],
                    module_path: String::new(),
                    via_reexport: true,
                }],
                context: EdgeContext::production(),
            },
        );
        assert!(g.importer_partition().providers.is_empty());
    }

    #[test]
    fn test_edges_are_ignored() {
        use crate::model::TestKind;
        let mut g = ArcGraph::new();
        let crate_idx = g.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let model = g.add_node(Node::Module {
            name: "model".into(),
            crate_idx,
        });
        let user = g.add_node(Node::Module {
            name: "user".into(),
            crate_idx,
        });
        g.add_edge(crate_idx, model, Edge::Contains);
        g.add_edge(crate_idx, user, Edge::Contains);
        g.add_edge(
            user,
            model,
            Edge::ModuleDep {
                locations: vec![SourceLocation {
                    file: "src/user.rs".into(),
                    line: 1,
                    symbols: vec!["Foo".to_owned()],
                    module_path: String::new(),
                    via_reexport: false,
                }],
                context: EdgeContext::test(TestKind::Unit),
            },
        );
        assert!(g.importer_partition().providers.is_empty());
    }

    #[test]
    fn common_ancestor_outside_provider_subtree() {
        // model provider; analyze has children parser & reexport, both import Foo.
        let (g, idx) = nested(
            &["model", "analyze", "parser", "reexport"],
            &[usize::MAX, usize::MAX, 1, 1],
            &[(2, 0, &["Foo"]), (3, 0, &["Foo"])],
        );
        let provider = &g.importer_partition().providers[0];
        assert_eq!(
            provider.consumer_groups,
            vec![ConsumerGroup {
                consumers: vec![idx[2], idx[3]],
                symbols: vec!["Foo".to_owned()],
                locality: ConsumerLocality::CommonAncestor(idx[1]),
            }]
        );
    }

    #[test]
    fn no_common_module_ancestor_is_crate_wide() {
        // Two top-level consumers share only the crate root.
        let (g, _idx) = nested(
            &["model", "user", "admin"],
            &[usize::MAX, usize::MAX, usize::MAX],
            &[(1, 0, &["Foo"]), (2, 0, &["Foo"])],
        );
        let provider = &g.importer_partition().providers[0];
        assert_eq!(
            provider.consumer_groups[0].locality,
            ConsumerLocality::CrateWide
        );
    }

    #[test]
    fn ancestor_containing_provider_is_crate_wide() {
        // core provider; its own children a & b are the consumers.
        let (g, _idx) = nested(
            &["core", "a", "b"],
            &[usize::MAX, 0, 0],
            &[(1, 0, &["Foo"]), (2, 0, &["Foo"])],
        );
        let provider = &g.importer_partition().providers[0];
        assert_eq!(
            provider.consumer_groups[0].locality,
            ConsumerLocality::CrateWide
        );
    }
}
