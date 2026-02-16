//! Graph Types & Builder

use crate::model::{
    CrateInfo, DependencyKind, DependencyRef, EdgeContext, ModuleInfo, ModuleTree, SourceLocation,
    TestKind,
};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Node {
    Crate { name: String, path: PathBuf },
    Module { name: String, crate_idx: NodeIndex },
}

impl Node {
    pub fn is_crate(&self) -> bool {
        matches!(self, Node::Crate { .. })
    }

    pub fn name(&self) -> &str {
        match self {
            Node::Crate { name, .. } | Node::Module { name, .. } => name,
        }
    }
}

#[derive(Debug)]
pub enum Edge {
    CrateDep {
        context: EdgeContext,
    },
    ModuleDep {
        locations: Vec<SourceLocation>,
        context: EdgeContext,
    },
    Contains,
}

impl Edge {
    /// Returns the edge context, if this is a dependency edge (not Contains).
    pub fn context(&self) -> Option<&EdgeContext> {
        match self {
            Edge::CrateDep { context } | Edge::ModuleDep { context, .. } => Some(context),
            Edge::Contains => None,
        }
    }

    /// Whether this edge represents a production dependency.
    pub fn is_production(&self) -> bool {
        self.context()
            .is_some_and(|c| c.kind == DependencyKind::Production)
    }

    pub fn is_production_module_dep(&self) -> bool {
        matches!(self, Edge::ModuleDep { context, .. } if context.kind == DependencyKind::Production)
    }

    pub fn is_production_crate_dep(&self) -> bool {
        matches!(self, Edge::CrateDep { context } if context.kind == DependencyKind::Production)
    }

    pub fn is_test_crate_dep(&self) -> bool {
        matches!(self, Edge::CrateDep { context } if matches!(context.kind, DependencyKind::Test(_)))
    }
}

/// Directed dependency graph for workspace crates and modules.
///
/// Wraps `petgraph::DiGraph<Node, Edge>` with domain-specific methods for
/// dependency analysis, reachability, and layout ordering.
pub struct ArcGraph(DiGraph<Node, Edge>);

impl std::ops::Deref for ArcGraph {
    type Target = DiGraph<Node, Edge>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ArcGraph {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Default for ArcGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ArcGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ArcGraph")
            .field(&self.0.node_count())
            .field(&self.0.edge_count())
            .finish()
    }
}

impl ArcGraph {
    pub fn new() -> Self {
        Self(DiGraph::new())
    }

    /// Subgraph containing only Production ModuleDep edges, with node weights
    /// mapping back to original `NodeIndex` values.
    pub fn production_subgraph(&self) -> DiGraph<NodeIndex, ()> {
        self.filter_map(
            |idx, _| Some(idx),
            |_, edge| edge.is_production_module_dep().then_some(()),
        )
    }

    /// Return the crate node that owns `idx`. For `Node::Module` this is
    /// the stored `crate_idx`; for `Node::Crate` it is `idx` itself.
    pub fn owning_crate(&self, idx: NodeIndex) -> NodeIndex {
        match &self[idx] {
            Node::Module { crate_idx, .. } => *crate_idx,
            Node::Crate { .. } => idx,
        }
    }

    /// Compute the set of production-reachable crate nodes.
    ///
    /// A crate is reachable if:
    /// 1. It is an "anchor" — has Contains edges (= has modules to visualize), OR
    /// 2. It is transitively reachable from an anchor via production CrateDep edges.
    ///
    /// Crates not in this set are test infrastructure (dev-dep crates and their
    /// transitive production dependencies) and should be pruned from the layout.
    ///
    /// When test CrateDep edges exist (--include-tests), all crates are reachable.
    pub fn production_reachable(&self) -> HashSet<NodeIndex> {
        // If test CrateDep edges exist, all crates are reachable (no pruning)
        if self
            .edge_indices()
            .any(|edge_idx| self[edge_idx].is_test_crate_dep())
        {
            return self
                .node_indices()
                .filter(|&n| self[n].is_crate())
                .collect();
        }

        // Anchors: crates with Contains edges (= have modules to visualize)
        let anchors: HashSet<NodeIndex> = self
            .node_indices()
            .filter(|&node| self[node].is_crate())
            .filter(|&node| {
                self.edges(node)
                    .any(|edge| matches!(edge.weight(), Edge::Contains))
            })
            .collect();

        // Forward-BFS from anchors over production CrateDep edges
        let mut reachable = anchors.clone();
        let mut frontier: VecDeque<_> = anchors.into_iter().collect();
        while let Some(current) = frontier.pop_front() {
            for target in self
                .edges(current)
                .filter(|edge| edge.weight().is_production_crate_dep())
                .map(|edge| edge.target())
                .filter(|target| self[*target].is_crate())
            {
                if reachable.insert(target) {
                    frontier.push_back(target);
                }
            }
        }
        reachable
    }

    /// Collect all descendants of a node (including itself) via Contains edges.
    pub fn containment_subtree(&self, root: NodeIndex) -> HashSet<NodeIndex> {
        let mut subtree = HashSet::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if subtree.insert(node) {
                stack.extend(
                    self.edges(node)
                        .filter(|edge| matches!(edge.weight(), Edge::Contains))
                        .map(|edge| edge.target()),
                );
            }
        }
        subtree
    }

    /// Whether `parent` has a `Contains` edge pointing to `child`.
    pub fn contains_child(&self, parent: NodeIndex, child: NodeIndex) -> bool {
        self.edges(parent)
            .any(|edge| edge.target() == child && matches!(edge.weight(), Edge::Contains))
    }

    /// Build a map from child → parent for all `Contains` edges.
    pub fn parent_map(&self) -> HashMap<NodeIndex, NodeIndex> {
        self.edge_indices()
            .filter(|&edge_idx| matches!(self[edge_idx], Edge::Contains))
            .map(|edge_idx| {
                let (parent, child) = self.edge_endpoints(edge_idx).unwrap();
                (child, parent)
            })
            .collect()
    }

    /// Build a unified graph from crate and module analysis data.
    pub fn build(crates: &[CrateInfo], modules: &[ModuleTree]) -> Self {
        let mut builder = GraphBuilder::new();
        builder.add_crates(crates);
        builder.add_modules(modules);
        builder.add_crate_deps(crates);
        builder.add_module_deps();
        builder.graph
    }
}

struct GraphBuilder {
    graph: ArcGraph,
    crate_map: HashMap<String, NodeIndex>,
    module_map: HashMap<String, NodeIndex>,
    module_deps: Vec<(String, Vec<DependencyRef>)>,
}

impl GraphBuilder {
    fn new() -> Self {
        Self {
            graph: ArcGraph::new(),
            crate_map: HashMap::new(),
            module_map: HashMap::new(),
            module_deps: Vec::new(),
        }
    }

    fn add_crates(&mut self, crates: &[CrateInfo]) {
        self.crate_map = crates
            .iter()
            .map(|crate_| {
                let idx = self.graph.add_node(Node::Crate {
                    name: crate_.name.clone(),
                    path: crate_.path.clone(),
                });
                (crate_.name.clone(), idx)
            })
            .collect();
    }

    fn add_modules(&mut self, modules: &[ModuleTree]) {
        for module_tree in modules {
            let Some(crate_idx) = self.resolve_node(&module_tree.root.name) else {
                continue;
            };

            self.stash_deps(&module_tree.root.name, &module_tree.root.dependencies);

            for child in &module_tree.root.children {
                self.add_modules_recursive(child, crate_idx, crate_idx);
            }
        }
    }

    fn stash_deps(&mut self, path: &str, deps: &[DependencyRef]) {
        if !deps.is_empty() {
            self.module_deps.push((path.to_owned(), deps.to_vec()));
        }
    }

    fn add_modules_recursive(
        &mut self,
        module: &ModuleInfo,
        crate_idx: NodeIndex,
        parent_idx: NodeIndex,
    ) {
        let module_idx = self.graph.add_node(Node::Module {
            name: module.name.clone(),
            crate_idx,
        });
        self.graph.add_edge(parent_idx, module_idx, Edge::Contains);
        self.module_map.insert(module.full_path.clone(), module_idx);

        self.stash_deps(&module.full_path, &module.dependencies);

        for child in &module.children {
            self.add_modules_recursive(child, crate_idx, module_idx);
        }
    }

    fn add_crate_deps(&mut self, crates: &[CrateInfo]) {
        for crate_info in crates {
            let Some(&from_idx) = self.crate_map.get(&crate_info.name) else {
                continue;
            };
            let prod = crate_info
                .dependencies
                .iter()
                .map(|dep| (dep, EdgeContext::production()));
            let dev = crate_info
                .dev_dependencies
                .iter()
                .map(|dep| (dep, EdgeContext::test(TestKind::Unit)));
            prod.chain(dev)
                .filter_map(|(name, ctx)| Some((self.crate_map.get(name)?, ctx)))
                .for_each(|(&to_idx, context)| {
                    self.graph
                        .add_edge(from_idx, to_idx, Edge::CrateDep { context });
                });
        }
    }

    fn add_module_deps(&mut self) {
        // Clone to avoid borrow conflict (self.module_deps read vs self.resolve_node)
        let module_deps: Vec<_> = self.module_deps.drain(..).collect();

        for (from_path, deps) in &module_deps {
            let Some(from_idx) = self.resolve_node(from_path) else {
                continue;
            };

            // Group deps by module_target to aggregate symbols into one edge.
            // Context is derived from the group: Production if any dep is Production,
            // otherwise Test. This ensures at most one edge per (from, to) node pair,
            // which the rendering pipeline requires (edge_id = "from-to").
            let mut grouped: BTreeMap<String, Vec<&DependencyRef>> = BTreeMap::new();
            for dep_ref in deps {
                grouped
                    .entry(dep_ref.module_target())
                    .or_default()
                    .push(dep_ref);
            }

            let resolved: Vec<_> = grouped
                .into_iter()
                .filter_map(|(target, target_deps)| {
                    let to_idx = self.resolve_node(&target)?;
                    (from_idx != to_idx).then_some((to_idx, target, target_deps))
                })
                .collect();

            for (to_idx, target, target_deps) in resolved {
                let context = aggregate_context(&target_deps);
                let locations = build_source_locations(&target_deps, &target);
                self.graph
                    .add_edge(from_idx, to_idx, Edge::ModuleDep { locations, context });
            }
        }
    }

    fn resolve_node(&self, name: &str) -> Option<NodeIndex> {
        self.module_map
            .get(name)
            .or_else(|| self.crate_map.get(name))
            .or_else(|| self.crate_map.get(&name.replace('_', "-")))
            .copied()
    }
}

fn build_source_locations(target_deps: &[&DependencyRef], target: &str) -> Vec<SourceLocation> {
    debug_assert!(!target_deps.is_empty(), "grouped deps must be non-empty");
    let module_path = match target_deps[0].target_module.as_str() {
        "" => target.to_owned(),
        path => path.to_owned(),
    };
    let mut by_line: BTreeMap<(PathBuf, usize), Vec<String>> = BTreeMap::new();
    for dep in target_deps {
        let entry = by_line
            .entry((dep.source_file.clone(), dep.line))
            .or_default();
        if let Some(item) = &dep.target_item {
            entry.push(item.clone());
        }
    }
    by_line
        .into_iter()
        .map(|((file, line), symbols)| SourceLocation {
            file,
            line,
            symbols,
            module_path: module_path.clone(),
        })
        .collect()
}

fn aggregate_context(deps: &[&DependencyRef]) -> EdgeContext {
    debug_assert!(!deps.is_empty(), "grouped deps must be non-empty");
    if deps
        .iter()
        .any(|dep| dep.context.kind == DependencyKind::Production)
    {
        EdgeContext::production()
    } else {
        deps[0].context.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CrateInfo, DependencyRef, ModuleInfo, ModuleTree};
    use std::path::PathBuf;

    // -- Construction helpers --

    fn crate_(name: &str) -> CrateInfo {
        CrateInfo {
            name: name.into(),
            path: format!("/path/to/{name}").into(),
            dependencies: vec![],
            dev_dependencies: vec![],
        }
    }

    fn crate_with_deps(name: &str, deps: &[&str]) -> CrateInfo {
        CrateInfo {
            dependencies: deps.iter().map(|&s| s.into()).collect(),
            ..crate_(name)
        }
    }

    fn module(name: &str, full_path: &str) -> ModuleInfo {
        ModuleInfo {
            name: name.into(),
            full_path: full_path.into(),
            children: vec![],
            dependencies: vec![],
        }
    }

    fn dep(target_crate: &str, target_module: &str, file: &str, line: usize) -> DependencyRef {
        DependencyRef {
            target_crate: target_crate.into(),
            target_module: target_module.into(),
            target_item: None,
            source_file: file.into(),
            line,
            context: EdgeContext::production(),
        }
    }

    fn tree(root: ModuleInfo) -> ModuleTree {
        ModuleTree { root }
    }

    // -- Edge-query helpers --

    fn count_edges(graph: &ArcGraph) -> (usize, usize, usize) {
        graph.edge_indices().fold(
            (0, 0, 0),
            |(crate_dep_count, module_dep_count, contains_count), edge_idx| match graph[edge_idx] {
                Edge::CrateDep { .. } => (crate_dep_count + 1, module_dep_count, contains_count),
                Edge::ModuleDep { .. } => (crate_dep_count, module_dep_count + 1, contains_count),
                Edge::Contains => (crate_dep_count, module_dep_count, contains_count + 1),
            },
        )
    }

    fn find_module_dep<'a>(
        graph: &'a ArcGraph,
        from_name: &str,
        to_name: &str,
    ) -> Option<(&'a EdgeContext, &'a [SourceLocation])> {
        graph
            .edge_indices()
            .find_map(|edge_idx| match &graph[edge_idx] {
                Edge::ModuleDep { context, locations } => {
                    let (from_node, to_node) = graph.edge_endpoints(edge_idx).unwrap();
                    (graph[from_node].name() == from_name && graph[to_node].name() == to_name)
                        .then_some((context, locations.as_slice()))
                }
                _ => None,
            })
    }

    // -- Tests --

    #[test]
    fn test_build_graph_single_crate() {
        let graph = ArcGraph::build(&[crate_("my_crate")], &[]);
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_build_graph_with_modules() {
        let crates = vec![crate_("my_crate")];
        let modules = vec![tree(ModuleInfo {
            children: vec![module("foo", "crate::foo"), module("bar", "crate::bar")],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&crates, &modules);
        assert_eq!(graph.node_count(), 3);
        let (cd, md, c) = count_edges(&graph);
        assert_eq!((cd, md, c), (0, 0, 2));
    }

    #[test]
    fn test_build_graph_crate_deps() {
        let crates = vec![crate_with_deps("crate_a", &["crate_b"]), crate_("crate_b")];
        let graph = ArcGraph::build(&crates, &[]);
        assert_eq!(graph.node_count(), 2);
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 1);
    }

    #[test]
    fn test_build_graph_module_deps() {
        let crates = vec![crate_("my_crate")];
        let modules = vec![tree(ModuleInfo {
            children: vec![
                module("foo", "crate::foo"),
                ModuleInfo {
                    dependencies: vec![dep("crate", "foo", "src/bar.rs", 1)],
                    ..module("bar", "crate::bar")
                },
            ],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&crates, &modules);
        assert_eq!(graph.node_count(), 3);
        let (cd, md, c) = count_edges(&graph);
        assert_eq!((cd, md, c), (0, 1, 2));
    }

    #[test]
    fn test_build_graph_inter_crate_module_deps() {
        let crates = vec![crate_with_deps("crate_a", &["crate_b"]), crate_("crate_b")];
        let modules = vec![
            tree(ModuleInfo {
                children: vec![ModuleInfo {
                    dependencies: vec![dep("crate_b", "gamma", "src/beta.rs", 1)],
                    ..module("beta", "crate_a::beta")
                }],
                ..module("crate_a", "crate_a")
            }),
            tree(ModuleInfo {
                children: vec![module("gamma", "crate_b::gamma")],
                ..module("crate_b", "crate_b")
            }),
        ];
        let graph = ArcGraph::build(&crates, &modules);
        assert_eq!(graph.node_count(), 4);
        let (cd, md, c) = count_edges(&graph);
        assert_eq!((cd, md, c), (1, 1, 2));
        let (_, locs) =
            find_module_dep(&graph, "beta", "gamma").expect("expected ModuleDep beta→gamma");
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].file, PathBuf::from("src/beta.rs"));
        assert_eq!(locs[0].line, 1);
    }

    #[test]
    fn test_root_dependencies_in_module_deps() {
        let crates = vec![crate_("crate_a")];
        let modules = vec![tree(ModuleInfo {
            children: vec![module("gamma", "crate_a::gamma")],
            dependencies: vec![dep("crate_a", "gamma", "src/lib.rs", 5)],
            ..module("crate_a", "crate_a")
        })];
        let graph = ArcGraph::build(&crates, &modules);
        let (_, locs) =
            find_module_dep(&graph, "crate_a", "gamma").expect("expected ModuleDep root→gamma");
        assert_eq!(locs[0].file, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn test_module_dep_to_crate_node() {
        let crates = vec![crate_with_deps("crate_a", &["crate_b"]), crate_("crate_b")];
        let modules = vec![
            tree(ModuleInfo {
                children: vec![ModuleInfo {
                    dependencies: vec![DependencyRef {
                        target_item: Some("Widget".into()),
                        ..dep("crate_b", "", "src/beta.rs", 3)
                    }],
                    ..module("beta", "crate_a::beta")
                }],
                ..module("crate_a", "crate_a")
            }),
            tree(module("crate_b", "crate_b")),
        ];
        let graph = ArcGraph::build(&crates, &modules);
        let (_, locs) = find_module_dep(&graph, "beta", "crate_b")
            .expect("expected ModuleDep from beta to crate_b");
        assert_eq!(locs[0].module_path, "crate_b");
        assert_eq!(locs[0].symbols, vec!["Widget"]);
    }

    #[test]
    fn test_root_dep_to_module() {
        let crates = vec![crate_with_deps("crate_a", &["crate_b"]), crate_("crate_b")];
        let modules = vec![
            tree(ModuleInfo {
                dependencies: vec![dep("crate_b", "gamma", "src/lib.rs", 2)],
                ..module("crate_a", "crate_a")
            }),
            tree(ModuleInfo {
                children: vec![module("gamma", "crate_b::gamma")],
                ..module("crate_b", "crate_b")
            }),
        ];
        let graph = ArcGraph::build(&crates, &modules);
        let (_, locs) =
            find_module_dep(&graph, "crate_a", "gamma").expect("expected ModuleDep root→gamma");
        assert_eq!(locs[0].file, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn test_root_dep_to_crate_node() {
        let crates = vec![crate_with_deps("crate_a", &["crate_b"]), crate_("crate_b")];
        let modules = vec![
            tree(ModuleInfo {
                dependencies: vec![DependencyRef {
                    target_item: Some("Config".into()),
                    ..dep("crate_b", "", "src/lib.rs", 1)
                }],
                ..module("crate_a", "crate_a")
            }),
            tree(module("crate_b", "crate_b")),
        ];
        let graph = ArcGraph::build(&crates, &modules);
        let (_, locs) = find_module_dep(&graph, "crate_a", "crate_b")
            .expect("expected ModuleDep crate_a→crate_b");
        assert_eq!(locs[0].module_path, "crate_b");
        assert_eq!(locs[0].symbols, vec!["Config"]);
    }

    #[test]
    fn test_cfg_test_dep_creates_test_edge() {
        let crates = vec![crate_("my_crate")];
        let modules = vec![tree(ModuleInfo {
            children: vec![
                module("foo", "crate::foo"),
                ModuleInfo {
                    dependencies: vec![DependencyRef {
                        target_item: Some("helper".into()),
                        context: EdgeContext::test(TestKind::Unit),
                        ..dep("crate", "foo", "src/bar.rs", 5)
                    }],
                    ..module("bar", "crate::bar")
                },
            ],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&crates, &modules);
        let (ctx, _) = find_module_dep(&graph, "bar", "foo").expect("expected ModuleDep bar→foo");
        assert_eq!(*ctx, EdgeContext::test(TestKind::Unit));
    }

    #[test]
    fn test_mixed_context_merges_into_production_edge() {
        let crates = vec![crate_("my_crate")];
        let modules = vec![tree(ModuleInfo {
            children: vec![
                module("foo", "crate::foo"),
                ModuleInfo {
                    dependencies: vec![
                        DependencyRef {
                            target_item: Some("run".into()),
                            ..dep("crate", "foo", "src/bar.rs", 1)
                        },
                        DependencyRef {
                            target_item: Some("test_helper".into()),
                            context: EdgeContext::test(TestKind::Unit),
                            ..dep("crate", "foo", "src/bar.rs", 10)
                        },
                    ],
                    ..module("bar", "crate::bar")
                },
            ],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&crates, &modules);
        let (ctx, locs) =
            find_module_dep(&graph, "bar", "foo").expect("expected ModuleDep bar→foo");
        assert_eq!(*ctx, EdgeContext::production());
        assert_eq!(locs.len(), 2);
    }
}
