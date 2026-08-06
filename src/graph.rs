//! Graph Types & Builder

use crate::analyze::externals::ExternalsResult;
use crate::model::{
    CrateInfo, DependencyKind, DependencyRef, EdgeContext, ModuleInfo, ModuleTree, SourceLocation,
    TestKind, normalize_crate_name,
};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Node {
    Crate {
        name: String,
        path: PathBuf,
    },
    Module {
        name: String,
        crate_idx: NodeIndex,
    },
    ExternalCrate {
        name: String,
        version: String,
        package_id: String,
        is_direct_dependency: bool,
    },
}

impl Node {
    #[must_use]
    pub fn is_crate(&self) -> bool {
        matches!(self, Node::Crate { .. })
    }

    #[must_use]
    pub fn is_external(&self) -> bool {
        matches!(self, Node::ExternalCrate { .. })
    }

    #[must_use]
    pub fn is_module(&self) -> bool {
        matches!(self, Node::Module { .. })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Node::Crate { name, .. }
            | Node::Module { name, .. }
            | Node::ExternalCrate { name, .. } => name,
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
    /// A workspace dev-dependency the current view does not show (no
    /// `--include-tests`). Nothing renders it; reachability needs it to tell
    /// test infrastructure from a crate that nobody depends on.
    DevDep,
}

impl Edge {
    /// Returns the edge context, if this is a dependency edge (not Contains).
    #[must_use]
    pub fn context(&self) -> Option<&EdgeContext> {
        match self {
            Edge::CrateDep { context } | Edge::ModuleDep { context, .. } => Some(context),
            Edge::Contains | Edge::DevDep => None,
        }
    }

    /// Whether this edge is a crate-level dependency, shown or not.
    #[must_use]
    pub fn is_crate_dep(&self) -> bool {
        matches!(self, Edge::CrateDep { .. } | Edge::DevDep)
    }

    /// Whether this edge represents a production dependency.
    #[must_use]
    pub fn is_production(&self) -> bool {
        self.context()
            .is_some_and(|c| c.kind == DependencyKind::Production)
    }

    #[must_use]
    pub fn is_production_module_dep(&self) -> bool {
        matches!(self, Edge::ModuleDep { context, .. } if context.kind == DependencyKind::Production)
    }

    /// Whether this is a production `ModuleDep` whose references are ALL
    /// `pub use` re-exports. Such edges republish names without behavioral
    /// coupling and are excluded from the logic subgraph (ADR-022).
    #[must_use]
    pub fn is_reexport_module_dep(&self) -> bool {
        matches!(
            self,
            Edge::ModuleDep { locations, context }
                if context.kind == DependencyKind::Production
                    && !locations.is_empty()
                    && locations.iter().all(|loc| loc.via_reexport)
        )
    }

    #[must_use]
    pub fn is_production_crate_dep(&self) -> bool {
        matches!(self, Edge::CrateDep { context } if context.kind == DependencyKind::Production)
    }

    #[must_use]
    pub fn is_test_crate_dep(&self) -> bool {
        matches!(self, Edge::CrateDep { context } if matches!(context.kind, DependencyKind::Test(_)))
    }
}

/// Whether `pub use` re-export edges take part in cycle detection. Excluded by
/// default (ADR-022): a re-export passes a name on rather than depending on it.
/// `--include-reexports` maps to `Included`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reexports {
    Included,
    Excluded,
}

impl From<bool> for Reexports {
    fn from(include_reexports: bool) -> Self {
        if include_reexports {
            Reexports::Included
        } else {
            Reexports::Excluded
        }
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
    #[must_use]
    pub fn new() -> Self {
        Self(DiGraph::new())
    }

    /// Subgraph containing Production `ModuleDep` edges, with node weights
    /// mapping back to original `NodeIndex` values. `Reexports::Excluded` drops
    /// pure re-export edges as well.
    #[must_use]
    pub fn production_subgraph(&self, reexports: Reexports) -> DiGraph<NodeIndex, ()> {
        self.filter_map(
            |idx, _| Some(idx),
            |_, edge| {
                let keep = edge.is_production_module_dep()
                    && (reexports == Reexports::Included || !edge.is_reexport_module_dep());
                keep.then_some(())
            },
        )
    }

    /// Return the crate node that owns `idx`. For `Node::Module` this is
    /// the stored `crate_idx`; for `Node::Crate` it is `idx` itself.
    #[must_use]
    pub fn owning_crate(&self, idx: NodeIndex) -> NodeIndex {
        match &self[idx] {
            Node::Module { crate_idx, .. } => *crate_idx,
            Node::Crate { .. } | Node::ExternalCrate { .. } => idx,
        }
    }

    /// Fully-qualified name of a node: `<crate>::<module::path>`.
    ///
    /// Reconstructs the module path by walking `Contains` edges up to the
    /// owning crate, so distinct modules sharing a leaf name (e.g. `error` and
    /// `device::error`) stay distinguishable. The crate segment is the Cargo
    /// package name (dash form). A node with no incoming `Contains` edge
    /// degrades to its leaf name.
    #[must_use]
    pub fn qualified_name(&self, idx: NodeIndex) -> String {
        let mut segments = vec![self[idx].name().to_string()];
        let mut current = idx;
        let mut crate_name = None;
        while let Some(edge) = self
            .edges_directed(current, petgraph::Direction::Incoming)
            .find(|edge| matches!(edge.weight(), Edge::Contains))
        {
            let parent = edge.source();
            if self[parent].is_crate() {
                crate_name = Some(self[parent].name());
                break;
            }
            segments.push(self[parent].name().to_string());
            current = parent;
        }
        segments.reverse();
        let path = segments.join("::");
        match crate_name {
            Some(crate_name) => format!("{crate_name}::{path}"),
            None => path,
        }
    }

    /// Compute the set of production-reachable crate nodes.
    ///
    /// A crate is reachable if:
    /// 1. It is an "anchor" — has Contains edges (= has modules to visualize),
    ///    or nobody depends on it while it depends on production code itself
    ///    (a workspace entry point, typically a thin binary), OR
    /// 2. It is transitively reachable from an anchor via production `CrateDep` edges.
    ///
    /// Crates not in this set are test infrastructure (dev-dep crates and their
    /// transitive production dependencies) and should be pruned from the layout.
    /// An incoming [`Edge::DevDep`] is what separates a test helper from an entry
    /// point: both are depended upon by nothing that ships.
    ///
    /// When test `CrateDep` edges exist (--include-tests), all crates are reachable.
    #[must_use]
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

        let all_crates: HashSet<NodeIndex> = self
            .node_indices()
            .filter(|&node| self[node].is_crate())
            .collect();

        // Pure crate-level diagram (no crate has submodules): all crates are anchors.
        // Mixed diagram: only crates with Contains edges are anchors; single-file
        // crates become reachable via BFS if a production dep points to them.
        let has_any_contains = all_crates.iter().any(|&node| {
            self.edges(node)
                .any(|edge| matches!(edge.weight(), Edge::Contains))
        });
        let anchors: HashSet<NodeIndex> = if has_any_contains {
            all_crates
                .iter()
                .copied()
                .filter(|&node| {
                    self.edges(node)
                        .any(|edge| matches!(edge.weight(), Edge::Contains))
                        || self.is_entry_point(node)
                })
                .collect()
        } else {
            all_crates
        };

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

    /// Whether `node` is a workspace entry point: no crate depends on it, and it
    /// depends on production code itself. A crate that only tests depend on fails
    /// the first condition, a crate that only tests use fails the second.
    fn is_entry_point(&self, node: NodeIndex) -> bool {
        let depended_upon = self
            .edges_directed(node, petgraph::Direction::Incoming)
            .any(|edge| edge.weight().is_crate_dep());
        let uses_production = self
            .edges(node)
            .any(|edge| edge.weight().is_production_crate_dep() && self[edge.target()].is_crate());
        !depended_upon && uses_production
    }

    /// Collect all descendants of a node (including itself) via Contains edges.
    #[must_use]
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
    #[must_use]
    pub fn contains_child(&self, parent: NodeIndex, child: NodeIndex) -> bool {
        self.edges(parent)
            .any(|edge| edge.target() == child && matches!(edge.weight(), Edge::Contains))
    }

    /// Build a map from child → parent for all `Contains` edges.
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn parent_map(&self) -> HashMap<NodeIndex, NodeIndex> {
        self.edge_indices()
            .filter(|&edge_idx| matches!(self[edge_idx], Edge::Contains))
            .map(|edge_idx| {
                let (parent, child) = self.edge_endpoints(edge_idx).expect("edge should exist");
                (child, parent)
            })
            .collect()
    }

    /// Deepest module that contains every node in `nodes` via `Contains` edges,
    /// or `None` when they share only the crate root (or span several crates).
    /// The crate root itself is never returned: it is not an actionable home.
    #[must_use]
    pub fn deepest_common_module(&self, nodes: &[NodeIndex]) -> Option<NodeIndex> {
        // Module ancestors of `start`, deepest first, stopping below the crate.
        let chain = |start: NodeIndex| -> Vec<NodeIndex> {
            let mut out = Vec::new();
            let mut node = start;
            while self[node].is_module() {
                out.push(node);
                match self
                    .edges_directed(node, petgraph::Direction::Incoming)
                    .find(|edge| matches!(edge.weight(), Edge::Contains))
                {
                    Some(edge) => node = edge.source(),
                    None => break,
                }
            }
            out
        };
        let (first, rest) = nodes.split_first()?;
        let others: Vec<HashSet<NodeIndex>> = rest
            .iter()
            .map(|&n| chain(n).into_iter().collect())
            .collect();
        chain(*first)
            .into_iter()
            .find(|node| others.iter().all(|set| set.contains(node)))
    }

    /// Build a unified graph from crate and module analysis data.
    /// `include_tests` decides whether dev-dependencies become shown test edges
    /// or the invisible [`Edge::DevDep`].
    #[must_use]
    pub(crate) fn build(
        crates: &[CrateInfo],
        modules: &[ModuleTree],
        externals: Option<&ExternalsResult>,
        include_tests: bool,
    ) -> Self {
        let mut builder = GraphBuilder::new();
        builder.add_crates(crates);
        builder.add_modules(modules);
        builder.add_crate_deps(crates, include_tests);
        builder.add_module_deps();
        if let Some(ext) = externals {
            builder.add_externals(ext);
        }
        builder.graph
    }
}

struct GraphBuilder {
    graph: ArcGraph,
    /// Keyed by `normalize_crate_name`; dependency names arrive underscored.
    crate_map: HashMap<String, NodeIndex>,
    module_map: HashMap<String, NodeIndex>,
    external_map: HashMap<String, NodeIndex>,
    module_deps: Vec<(String, Vec<DependencyRef>)>,
}

impl GraphBuilder {
    fn new() -> Self {
        Self {
            graph: ArcGraph::new(),
            crate_map: HashMap::new(),
            module_map: HashMap::new(),
            external_map: HashMap::new(),
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
                (normalize_crate_name(&crate_.name), idx)
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

    fn add_crate_deps(&mut self, crates: &[CrateInfo], include_tests: bool) {
        for crate_info in crates {
            let Some(&from_idx) = self.crate_map.get(&normalize_crate_name(&crate_info.name))
            else {
                continue;
            };
            let prod = crate_info.dependencies.iter().map(|dep| {
                (
                    dep,
                    Edge::CrateDep {
                        context: EdgeContext::production(),
                    },
                )
            });
            let dev = crate_info.dev_dependencies.iter().map(|dep| {
                let edge = if include_tests {
                    Edge::CrateDep {
                        context: EdgeContext::test(TestKind::Unit),
                    }
                } else {
                    Edge::DevDep
                };
                (dep, edge)
            });
            prod.chain(dev)
                .filter_map(|(name, edge)| {
                    Some((self.crate_map.get(&normalize_crate_name(name))?, edge))
                })
                .for_each(|(&to_idx, edge)| {
                    self.graph.add_edge(from_idx, to_idx, edge);
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

    fn add_externals(&mut self, ext: &ExternalsResult) {
        /// Map `cargo_metadata` `DependencyKind`s to our `EdgeContext`.
        /// Dev-only deps get `test()`, everything else `production()`.
        fn edge_context_from_dep_kinds(kinds: &[cargo_metadata::DependencyKind]) -> EdgeContext {
            let has_normal = kinds
                .iter()
                .any(|k| matches!(k, cargo_metadata::DependencyKind::Normal));
            if has_normal {
                EdgeContext::production()
            } else {
                EdgeContext::test(TestKind::Unit)
            }
        }

        // Collect package IDs that are direct workspace dependencies for O(1) lookup.
        let direct_pkg_ids: HashSet<&str> = ext
            .workspace_deps
            .iter()
            .map(|dep| dep.external_pkg_id.as_str())
            .collect();

        // Add external crate nodes, build package_id -> NodeIndex map
        let mut pkg_index: HashMap<&str, NodeIndex> = HashMap::new();
        for info in &ext.crates {
            let idx = self.graph.add_node(Node::ExternalCrate {
                name: info.name.clone(),
                version: info.version.clone(),
                package_id: info.package_id.clone(),
                is_direct_dependency: direct_pkg_ids.contains(info.package_id.as_str()),
            });
            pkg_index.insert(&info.package_id, idx);
            self.external_map.insert(info.name.clone(), idx);
        }

        // Workspace -> external CrateDep edges
        for dep in &ext.workspace_deps {
            let Some(&ext_idx) = pkg_index.get(dep.external_pkg_id.as_str()) else {
                continue;
            };
            let context = edge_context_from_dep_kinds(&dep.dep_kinds);
            let Some(&ws_idx) = self
                .crate_map
                .get(&normalize_crate_name(&dep.workspace_crate))
            else {
                continue;
            };
            self.graph
                .add_edge(ws_idx, ext_idx, Edge::CrateDep { context });
        }

        // External -> external edges (only populated in transitive mode)
        for dep in &ext.external_deps {
            let from = pkg_index.get(dep.from_pkg_id.as_str());
            let to = pkg_index.get(dep.to_pkg_id.as_str());
            if let (Some(&from_idx), Some(&to_idx)) = (from, to) {
                let context = edge_context_from_dep_kinds(&dep.dep_kinds);
                self.graph
                    .add_edge(from_idx, to_idx, Edge::CrateDep { context });
            }
        }
    }

    fn resolve_node(&self, name: &str) -> Option<NodeIndex> {
        self.module_map
            .get(name)
            .or_else(|| self.crate_map.get(&normalize_crate_name(name)))
            .or_else(|| self.external_map.get(name))
            .or_else(|| self.external_map.get(&name.replace('_', "-")))
            .copied()
    }
}

fn build_source_locations(target_deps: &[&DependencyRef], target: &str) -> Vec<SourceLocation> {
    debug_assert!(!target_deps.is_empty(), "grouped deps must be non-empty");
    let module_path = match target_deps[0].target_module.as_str() {
        "" => target.to_owned(),
        path => path.to_owned(),
    };
    // Per (file, line): collect symbols and whether every ref there is a
    // re-export. `via_reexport` starts true and clears on the first non-re-export
    // ref, so a location is a pure re-export only if all its refs are.
    let mut by_line: BTreeMap<(PathBuf, usize), (Vec<String>, bool)> = BTreeMap::new();
    for dep in target_deps {
        let (symbols, via_reexport) = by_line
            .entry((dep.source_file.clone(), dep.line))
            .or_insert_with(|| (Vec::new(), true));
        if let Some(item) = &dep.target_item {
            symbols.push(item.clone());
        }
        *via_reexport &= dep.via_reexport;
    }
    by_line
        .into_iter()
        .map(|((file, line), (symbols, via_reexport))| SourceLocation {
            file,
            line,
            symbols,
            module_path: module_path.clone(),
            via_reexport,
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
    use crate::test_support::conventional_crate;
    use std::path::PathBuf;

    // -- Construction helpers --

    fn crate_(name: &str) -> CrateInfo {
        conventional_crate(name, format!("/path/to/{name}"))
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
            via_reexport: false,
        }
    }

    fn reexport_dep(
        target_crate: &str,
        target_module: &str,
        file: &str,
        line: usize,
    ) -> DependencyRef {
        DependencyRef {
            via_reexport: true,
            ..dep(target_crate, target_module, file, line)
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
                // Not a shown dependency; tests that care query it directly.
                Edge::DevDep => (crate_dep_count, module_dep_count, contains_count),
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
                    let (from_node, to_node) =
                        graph.edge_endpoints(edge_idx).expect("edge should exist");
                    (graph[from_node].name() == from_name && graph[to_node].name() == to_name)
                        .then_some((context, locations.as_slice()))
                }
                _ => None,
            })
    }

    // -- Tests --

    #[test]
    fn test_build_graph_single_crate() {
        let graph = ArcGraph::build(&[crate_("my_crate")], &[], None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
        assert_eq!(graph.node_count(), 3);
        let (cd, md, c) = count_edges(&graph);
        assert_eq!((cd, md, c), (0, 0, 2));
    }

    #[test]
    fn test_build_graph_crate_deps() {
        let crates = vec![crate_with_deps("crate_a", &["crate_b"]), crate_("crate_b")];
        let graph = ArcGraph::build(&crates, &[], None, false);
        assert_eq!(graph.node_count(), 2);
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 1);
    }

    #[test]
    fn test_build_graph_crate_deps_hyphenated_package_name() {
        let crates = vec![crate_with_deps("crate-a", &["crate_b"]), crate_("crate-b")];
        let graph = ArcGraph::build(&crates, &[], None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
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
    fn test_production_subgraph_excludes_pure_reexport_edges_when_excluded() {
        // foo re-exports from bar (pub use), bar uses foo (behavioral).
        // Reexports::Included keeps both edges → cycle; Reexports::Excluded drops
        // the pure re-export edge foo→bar → acyclic.
        let crates = vec![crate_("my_crate")];
        let modules = vec![tree(ModuleInfo {
            children: vec![
                ModuleInfo {
                    dependencies: vec![reexport_dep("crate", "bar", "src/foo.rs", 1)],
                    ..module("foo", "crate::foo")
                },
                ModuleInfo {
                    dependencies: vec![dep("crate", "foo", "src/bar.rs", 1)],
                    ..module("bar", "crate::bar")
                },
            ],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&crates, &modules, None, false);

        let included = graph.production_subgraph(Reexports::Included);
        assert_eq!(included.edge_count(), 2, "included keeps both edges");
        assert!(petgraph::algo::is_cyclic_directed(&included));

        let excluded = graph.production_subgraph(Reexports::Excluded);
        assert_eq!(
            excluded.edge_count(),
            1,
            "excluded drops the pure re-export edge"
        );
        assert!(
            !petgraph::algo::is_cyclic_directed(&excluded),
            "excluded subgraph is acyclic"
        );
    }

    #[test]
    fn test_root_dependencies_in_module_deps() {
        let crates = vec![crate_("crate_a")];
        let modules = vec![tree(ModuleInfo {
            children: vec![module("gamma", "crate_a::gamma")],
            dependencies: vec![dep("crate_a", "gamma", "src/lib.rs", 5)],
            ..module("crate_a", "crate_a")
        })];
        let graph = ArcGraph::build(&crates, &modules, None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
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
        let graph = ArcGraph::build(&crates, &modules, None, false);
        let (ctx, locs) =
            find_module_dep(&graph, "bar", "foo").expect("expected ModuleDep bar→foo");
        assert_eq!(*ctx, EdgeContext::production());
        assert_eq!(locs.len(), 2);
    }

    #[test]
    fn test_external_crate_node_properties() {
        let node = Node::ExternalCrate {
            name: "serde".into(),
            version: "1.0.0".into(),
            package_id: "serde 1.0.0 (registry+...)".into(),
            is_direct_dependency: true,
        };
        assert!(!node.is_crate());
        assert!(node.is_external());
        assert_eq!(node.name(), "serde");
    }

    #[test]
    fn test_production_reachable_excludes_external() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "my_crate".into(),
            path: "/path".into(),
        });
        let mod_idx = graph.add_node(Node::Module {
            name: "foo".into(),
            crate_idx,
        });
        graph.add_edge(crate_idx, mod_idx, Edge::Contains);
        let ext_idx = graph.add_node(Node::ExternalCrate {
            name: "serde".into(),
            version: "1.0.0".into(),
            package_id: "serde-pkg".into(),
            is_direct_dependency: true,
        });
        graph.add_edge(
            crate_idx,
            ext_idx,
            Edge::CrateDep {
                context: EdgeContext::production(),
            },
        );
        let reachable = graph.production_reachable();
        assert!(reachable.contains(&crate_idx));
        assert!(
            !reachable.contains(&ext_idx),
            "ExternalCrate should not be in production_reachable"
        );
    }

    #[test]
    fn test_production_reachable_crates_without_submodules() {
        let mut graph = ArcGraph::new();
        let a = graph.add_node(Node::Crate {
            name: "alpha".into(),
            path: "/path".into(),
        });
        let b = graph.add_node(Node::Crate {
            name: "beta".into(),
            path: "/path".into(),
        });
        graph.add_edge(
            a,
            b,
            Edge::CrateDep {
                context: EdgeContext::production(),
            },
        );
        // No Contains edges anywhere → pure crate-level diagram
        let reachable = graph.production_reachable();
        assert!(reachable.contains(&a), "alpha should be reachable");
        assert!(reachable.contains(&b), "beta should be reachable");
    }

    /// Crate with modules, a leaf binary depending on it, and a test helper the
    /// binary only pulls in for tests. The helper depends on production code
    /// itself, so only the incoming edges tell the two module-less crates apart.
    fn mixed_graph_with_leaf_and_helper() -> (ArcGraph, NodeIndex, NodeIndex) {
        let mut graph = ArcGraph::new();
        let lib = graph.add_node(Node::Crate {
            name: "lib".into(),
            path: "/path".into(),
        });
        let module = graph.add_node(Node::Module {
            name: "engine".into(),
            crate_idx: lib,
        });
        graph.add_edge(lib, module, Edge::Contains);

        let binary = graph.add_node(Node::Crate {
            name: "binary".into(),
            path: "/path".into(),
        });
        let helper = graph.add_node(Node::Crate {
            name: "helper".into(),
            path: "/path".into(),
        });
        for source in [binary, helper] {
            graph.add_edge(
                source,
                lib,
                Edge::CrateDep {
                    context: EdgeContext::production(),
                },
            );
        }
        graph.add_edge(binary, helper, Edge::DevDep);
        (graph, binary, helper)
    }

    #[test]
    fn test_production_reachable_keeps_leaf_binary() {
        let (graph, binary, _) = mixed_graph_with_leaf_and_helper();
        assert!(
            graph.production_reachable().contains(&binary),
            "a binary nobody depends on is the workspace entry point, not test infrastructure"
        );
    }

    #[test]
    fn test_production_reachable_drops_dev_only_crate() {
        let (graph, _, helper) = mixed_graph_with_leaf_and_helper();
        assert!(
            !graph.production_reachable().contains(&helper),
            "a crate only reached through a dev-dependency is test infrastructure"
        );
    }

    #[test]
    fn test_owning_crate_external() {
        let mut graph = ArcGraph::new();
        let ext_idx = graph.add_node(Node::ExternalCrate {
            name: "serde".into(),
            version: "1.0.0".into(),
            package_id: "serde-pkg".into(),
            is_direct_dependency: true,
        });
        assert_eq!(graph.owning_crate(ext_idx), ext_idx);
    }

    #[test]
    fn test_qualified_name_disambiguates_same_leaf() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "my-crate".into(),
            path: "/my-crate".into(),
        });
        let top_store = graph.add_node(Node::Module {
            name: "store".into(),
            crate_idx,
        });
        graph.add_edge(crate_idx, top_store, Edge::Contains);
        let core = graph.add_node(Node::Module {
            name: "core".into(),
            crate_idx,
        });
        graph.add_edge(crate_idx, core, Edge::Contains);
        let core_store = graph.add_node(Node::Module {
            name: "store".into(),
            crate_idx,
        });
        graph.add_edge(core, core_store, Edge::Contains);

        assert_eq!(graph.qualified_name(top_store), "my-crate::store");
        assert_eq!(graph.qualified_name(core_store), "my-crate::core::store");
    }

    #[test]
    fn test_qualified_name_orphan_falls_back_to_leaf() {
        let mut graph = ArcGraph::new();
        let idx = graph.add_node(Node::Module {
            name: "lonely".into(),
            crate_idx: NodeIndex::new(0),
        });
        assert_eq!(graph.qualified_name(idx), "lonely");
    }

    #[test]
    fn test_qualified_name_crate_returns_bare_name() {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "my-crate".into(),
            path: "/my-crate".into(),
        });
        assert_eq!(graph.qualified_name(crate_idx), "my-crate");
    }

    #[test]
    fn test_build_graph_with_externals() {
        use crate::analyze::externals::*;
        use cargo_metadata::DependencyKind as DK;

        let crates = vec![crate_("my_crate")];
        let externals = ExternalsResult {
            crates: vec![
                ExternalCrateInfo {
                    name: "serde".into(),
                    version: "1.0.0".into(),
                    package_id: "serde-pkg".into(),
                },
                ExternalCrateInfo {
                    name: "tokio".into(),
                    version: "1.0.0".into(),
                    package_id: "tokio-pkg".into(),
                },
            ],
            workspace_deps: vec![WorkspaceExternalDep {
                workspace_crate: "my_crate".into(),
                external_pkg_id: "serde-pkg".into(),
                dep_kinds: vec![DK::Normal],
            }],
            external_deps: vec![ExternalDep {
                from_pkg_id: "serde-pkg".into(),
                to_pkg_id: "tokio-pkg".into(),
                dep_kinds: vec![DK::Normal],
            }],
            crate_name_map: std::collections::HashMap::new(),
        };
        let graph = ArcGraph::build(&crates, &[], Some(&externals), false);
        // 1 workspace + 2 external = 3 nodes
        assert_eq!(graph.node_count(), 3);
        // 1 workspace->serde + 1 serde->tokio = 2 CrateDep edges
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 2);
    }

    #[test]
    fn test_external_is_direct_dependency_flag() {
        use crate::analyze::externals::*;
        use cargo_metadata::DependencyKind as DK;

        let crates = vec![crate_("my_crate")];
        let externals = ExternalsResult {
            crates: vec![
                ExternalCrateInfo {
                    name: "serde".into(),
                    version: "1.0.0".into(),
                    package_id: "serde-pkg".into(),
                },
                ExternalCrateInfo {
                    name: "tokio".into(),
                    version: "1.0.0".into(),
                    package_id: "tokio-pkg".into(),
                },
            ],
            workspace_deps: vec![WorkspaceExternalDep {
                workspace_crate: "my_crate".into(),
                external_pkg_id: "serde-pkg".into(),
                dep_kinds: vec![DK::Normal],
            }],
            external_deps: vec![ExternalDep {
                from_pkg_id: "serde-pkg".into(),
                to_pkg_id: "tokio-pkg".into(),
                dep_kinds: vec![DK::Normal],
            }],
            crate_name_map: std::collections::HashMap::new(),
        };
        let graph = ArcGraph::build(&crates, &[], Some(&externals), false);

        // serde is a direct workspace dependency
        let serde = graph
            .node_indices()
            .find(|&idx| graph[idx].name() == "serde")
            .expect("serde node should exist");
        assert!(
            matches!(
                &graph[serde],
                Node::ExternalCrate {
                    is_direct_dependency: true,
                    ..
                }
            ),
            "serde should be a direct dependency"
        );

        // tokio is only reachable transitively (serde -> tokio)
        let tokio = graph
            .node_indices()
            .find(|&idx| graph[idx].name() == "tokio")
            .expect("tokio node should exist");
        assert!(
            matches!(
                &graph[tokio],
                Node::ExternalCrate {
                    is_direct_dependency: false,
                    ..
                }
            ),
            "tokio should be a transitive dependency"
        );
    }

    #[test]
    fn test_build_graph_externals_none() {
        let crates = vec![crate_with_deps("a", &["b"]), crate_("b")];
        let graph = ArcGraph::build(&crates, &[], None, false);
        assert_eq!(graph.node_count(), 2);
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 1);
    }

    #[test]
    fn test_resolve_node_finds_external() {
        use crate::analyze::externals::*;

        let crates = vec![crate_("my_crate")];
        let externals = ExternalsResult {
            crates: vec![ExternalCrateInfo {
                name: "serde".into(),
                version: "1.0.0".into(),
                package_id: "serde-pkg".into(),
            }],
            workspace_deps: vec![],
            external_deps: vec![],
            crate_name_map: std::collections::HashMap::new(),
        };
        let graph = ArcGraph::build(&crates, &[], Some(&externals), false);
        // Verify the external node exists
        let ext_node = graph
            .node_indices()
            .find(|&idx| graph[idx].name() == "serde");
        assert!(ext_node.is_some(), "should find serde external node");
        assert!(graph[ext_node.unwrap()].is_external());
    }
}
