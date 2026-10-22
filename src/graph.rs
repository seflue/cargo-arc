//! Graph Types & Builder

use crate::model::{
    Definition, DependencyRef, EdgeContext, EntryFile, ExternalsResult, ModuleInfo, ModuleTree,
    PackageInfo, SourceLocation, TestKind, UsageKind,
};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Node {
    Package {
        name: String,
        path: PathBuf,
        entry_files: Vec<EntryFile>,
        manifest: Option<PathBuf>,
    },
    Module {
        name: String,
        package_idx: NodeIndex,
        /// Absolute path of the declaring file, see `ModuleInfo::file`.
        file: Option<PathBuf>,
        /// The module's own definitions, see `ModuleInfo::definitions`.
        definitions: HashMap<String, Definition>,
    },
    ExternalPackage {
        name: String,
        version: String,
        package_id: String,
        is_direct_dependency: bool,
        entry_files: Vec<EntryFile>,
        manifest: Option<PathBuf>,
    },
}

impl Node {
    #[must_use]
    pub fn is_package(&self) -> bool {
        matches!(self, Node::Package { .. })
    }

    #[must_use]
    pub fn is_external(&self) -> bool {
        matches!(self, Node::ExternalPackage { .. })
    }

    #[must_use]
    pub fn is_module(&self) -> bool {
        matches!(self, Node::Module { .. })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Node::Package { name, .. }
            | Node::Module { name, .. }
            | Node::ExternalPackage { name, .. } => name,
        }
    }
}

#[derive(Debug)]
pub enum EdgeWeight {
    PackageDep {
        context: EdgeContext,
    },
    ModuleDep {
        locations: Vec<SourceLocation>,
        context: EdgeContext,
    },
    Contains,
    /// A workspace dev-dependency the current view does not show (no
    /// `--include-tests`). Nothing renders it; reachability needs it to tell
    /// test infrastructure from a package that nobody depends on.
    DevDep,
}

impl EdgeWeight {
    /// Returns the edge context, if this is a dependency edge (not Contains).
    #[must_use]
    pub fn context(&self) -> Option<&EdgeContext> {
        match self {
            EdgeWeight::PackageDep { context } | EdgeWeight::ModuleDep { context, .. } => {
                Some(context)
            }
            EdgeWeight::Contains | EdgeWeight::DevDep => None,
        }
    }

    /// Whether this edge is a package-level dependency, shown or not.
    #[must_use]
    pub fn is_package_dep(&self) -> bool {
        matches!(self, EdgeWeight::PackageDep { .. } | EdgeWeight::DevDep)
    }

    /// Whether this edge represents a production dependency.
    #[must_use]
    pub fn is_production(&self) -> bool {
        self.context()
            .is_some_and(|c| c.kind == UsageKind::Production)
    }

    #[must_use]
    pub fn is_production_module_dep(&self) -> bool {
        matches!(self, EdgeWeight::ModuleDep { context, .. } if context.kind == UsageKind::Production)
    }

    #[must_use]
    pub fn is_test_module_dep(&self) -> bool {
        matches!(self, EdgeWeight::ModuleDep { context, .. } if matches!(context.kind, UsageKind::Test(_)))
    }

    /// Whether this is a production `ModuleDep` whose references are ALL
    /// `pub use` re-exports. Such edges republish names without behavioral
    /// coupling and are excluded from the logic subgraph (ADR-022).
    #[must_use]
    pub fn is_reexport_module_dep(&self) -> bool {
        matches!(
            self,
            EdgeWeight::ModuleDep { locations, context }
                if context.kind == UsageKind::Production
                    && !locations.is_empty()
                    && locations.iter().all(|loc| loc.via_reexport)
        )
    }

    #[must_use]
    pub fn is_production_package_dep(&self) -> bool {
        matches!(self, EdgeWeight::PackageDep { context } if context.kind == UsageKind::Production)
    }

    #[must_use]
    pub fn is_test_package_dep(&self) -> bool {
        matches!(self, EdgeWeight::PackageDep { context } if matches!(context.kind, UsageKind::Test(_)))
    }

    /// Whether this edge belongs to [`ArcGraph::production_subgraph`] under
    /// `reexports`: a production `ModuleDep`, and not a pure re-export unless
    /// those are included.
    #[must_use]
    pub fn in_production_subgraph(&self, reexports: Reexports) -> bool {
        self.is_production_module_dep()
            && (reexports == Reexports::Included || !self.is_reexport_module_dep())
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

/// Directed dependency graph for workspace packages and modules.
///
/// Wraps `petgraph::DiGraph<Node, EdgeWeight>` with domain-specific methods for
/// dependency analysis, reachability, and layout ordering.
pub struct ArcGraph(DiGraph<Node, EdgeWeight>);

impl std::ops::Deref for ArcGraph {
    type Target = DiGraph<Node, EdgeWeight>;
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
            |_, edge| edge.in_production_subgraph(reexports).then_some(()),
        )
    }

    /// Return the package node that owns `idx`. For `Node::Module` this is
    /// the stored `package_idx`; for `Node::Package` it is `idx` itself.
    #[must_use]
    pub fn owning_package(&self, idx: NodeIndex) -> NodeIndex {
        match &self[idx] {
            Node::Module { package_idx, .. } => *package_idx,
            Node::Package { .. } | Node::ExternalPackage { .. } => idx,
        }
    }

    /// Fully-qualified name of a node: `<package>::<module::path>`.
    ///
    /// Reconstructs the module path by walking `Contains` edges up to the
    /// owning package, so distinct modules sharing a leaf name (e.g. `error` and
    /// `device::error`) stay distinguishable. The package segment is the Cargo
    /// package name (dash form). A node with no incoming `Contains` edge
    /// degrades to its leaf name.
    #[must_use]
    pub fn qualified_name(&self, idx: NodeIndex) -> String {
        let mut segments = vec![self[idx].name().to_string()];
        let mut current = idx;
        let mut package_name = None;
        while let Some(edge) = self
            .edges_directed(current, petgraph::Direction::Incoming)
            .find(|edge| matches!(edge.weight(), EdgeWeight::Contains))
        {
            let parent = edge.source();
            if self[parent].is_package() {
                package_name = Some(self[parent].name());
                break;
            }
            segments.push(self[parent].name().to_string());
            current = parent;
        }
        segments.reverse();
        let path = segments.join("::");
        match package_name {
            Some(package_name) => format!("{package_name}::{path}"),
            None => path,
        }
    }

    /// Compute the set of production-reachable package nodes.
    ///
    /// A package is reachable if:
    /// 1. It is an "anchor" — has Contains edges (= has modules to visualize),
    ///    or nobody depends on it while it depends on production code itself
    ///    (a workspace entry point, typically a thin binary), OR
    /// 2. It is transitively reachable from an anchor via production `PackageDep` edges.
    ///
    /// Packages not in this set are test infrastructure (dev-dep packages and their
    /// transitive production dependencies) and should be pruned from the layout.
    /// An incoming [`EdgeWeight::DevDep`] is what separates a test helper from an entry
    /// point: both are depended upon by nothing that ships.
    ///
    /// When test `PackageDep` edges exist (--include-tests), all packages are reachable.
    #[must_use]
    pub fn production_reachable(&self) -> HashSet<NodeIndex> {
        // If test PackageDep edges exist, all packages are reachable (no pruning)
        if self
            .edge_indices()
            .any(|edge_idx| self[edge_idx].is_test_package_dep())
        {
            return self
                .node_indices()
                .filter(|&n| self[n].is_package())
                .collect();
        }

        let all_packages: HashSet<NodeIndex> = self
            .node_indices()
            .filter(|&node| self[node].is_package())
            .collect();

        // Pure package-level diagram (no package has submodules): all packages are anchors.
        // Mixed diagram: only packages with Contains edges are anchors; single-file
        // packages become reachable via BFS if a production dep points to them.
        let has_any_contains = all_packages.iter().any(|&node| {
            self.edges(node)
                .any(|edge| matches!(edge.weight(), EdgeWeight::Contains))
        });
        let anchors: HashSet<NodeIndex> = if has_any_contains {
            all_packages
                .iter()
                .copied()
                .filter(|&node| {
                    self.edges(node)
                        .any(|edge| matches!(edge.weight(), EdgeWeight::Contains))
                        || self.is_entry_point(node)
                })
                .collect()
        } else {
            all_packages
        };

        // Forward-BFS from anchors over production PackageDep edges
        let mut reachable = anchors.clone();
        let mut frontier: VecDeque<_> = anchors.into_iter().collect();
        while let Some(current) = frontier.pop_front() {
            for target in self
                .edges(current)
                .filter(|edge| edge.weight().is_production_package_dep())
                .map(|edge| edge.target())
                .filter(|target| self[*target].is_package())
            {
                if reachable.insert(target) {
                    frontier.push_back(target);
                }
            }
        }
        reachable
    }

    /// Whether `node` is a workspace entry point: no package depends on it, and it
    /// depends on production code itself. A package that only tests depend on fails
    /// the first condition, a package that only tests use fails the second.
    fn is_entry_point(&self, node: NodeIndex) -> bool {
        let depended_upon = self
            .edges_directed(node, petgraph::Direction::Incoming)
            .any(|edge| edge.weight().is_package_dep());
        let uses_production = self.edges(node).any(|edge| {
            edge.weight().is_production_package_dep() && self[edge.target()].is_package()
        });
        !depended_upon && uses_production
    }

    /// Collect all descendants of a node (including itself) via Contains edges.
    #[must_use]
    pub fn containment_subtree(&self, start: NodeIndex) -> HashSet<NodeIndex> {
        let mut subtree = HashSet::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if subtree.insert(node) {
                stack.extend(
                    self.edges(node)
                        .filter(|edge| matches!(edge.weight(), EdgeWeight::Contains))
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
            .any(|edge| edge.target() == child && matches!(edge.weight(), EdgeWeight::Contains))
    }

    /// Build a map from child → parent for all `Contains` edges.
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn parent_map(&self) -> HashMap<NodeIndex, NodeIndex> {
        self.edge_indices()
            .filter(|&edge_idx| matches!(self[edge_idx], EdgeWeight::Contains))
            .map(|edge_idx| {
                let (parent, child) = self.edge_endpoints(edge_idx).expect("edge should exist");
                (child, parent)
            })
            .collect()
    }

    /// Deepest module that contains every node in `nodes` via `Contains` edges,
    /// or `None` when they share only the package root (or span several packages).
    /// The package root itself is never returned: it is not an actionable home.
    #[must_use]
    pub fn deepest_common_module(&self, nodes: &[NodeIndex]) -> Option<NodeIndex> {
        // Module ancestors of `start`, deepest first, stopping below the package.
        let chain = |start: NodeIndex| -> Vec<NodeIndex> {
            let mut out = Vec::new();
            let mut node = start;
            while self[node].is_module() {
                out.push(node);
                match self
                    .edges_directed(node, petgraph::Direction::Incoming)
                    .find(|edge| matches!(edge.weight(), EdgeWeight::Contains))
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

    /// Build a unified graph from package and module analysis data.
    /// `include_tests` decides whether dev-dependencies become shown test edges
    /// or the invisible [`EdgeWeight::DevDep`].
    #[must_use]
    pub(crate) fn build(
        packages: &[PackageInfo],
        modules: &[ModuleTree],
        externals: Option<&ExternalsResult>,
        include_tests: bool,
    ) -> Self {
        let mut builder = GraphBuilder::new();
        builder.add_packages(packages);
        builder.add_modules(modules);
        builder.add_package_deps(packages, include_tests);
        builder.add_module_deps();
        if let Some(ext) = externals {
            builder.add_externals(ext);
        }
        builder.graph
    }
}

struct GraphBuilder {
    graph: ArcGraph,
    /// Keyed by `PackageInfo::key`.
    package_map: HashMap<String, NodeIndex>,
    module_map: HashMap<String, NodeIndex>,
    external_map: HashMap<String, NodeIndex>,
    module_deps: Vec<(String, Vec<DependencyRef>)>,
}

impl GraphBuilder {
    fn new() -> Self {
        Self {
            graph: ArcGraph::new(),
            package_map: HashMap::new(),
            module_map: HashMap::new(),
            external_map: HashMap::new(),
            module_deps: Vec::new(),
        }
    }

    fn add_packages(&mut self, packages: &[PackageInfo]) {
        self.package_map = packages
            .iter()
            .map(|package| {
                let idx = self.graph.add_node(Node::Package {
                    name: package.name.clone(),
                    path: package.path.clone(),
                    entry_files: package.entry_files.clone(),
                    manifest: package.manifest.clone(),
                });
                (package.key.clone(), idx)
            })
            .collect();
    }

    fn add_modules(&mut self, modules: &[ModuleTree]) {
        for module_tree in modules {
            let Some(package_idx) = self.resolve_node(&module_tree.root.name) else {
                continue;
            };

            self.stash_deps(&module_tree.root.name, &module_tree.root.dependencies);

            for child in &module_tree.root.children {
                self.add_modules_recursive(child, package_idx, package_idx);
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
        package_idx: NodeIndex,
        parent_idx: NodeIndex,
    ) {
        let module_idx = self.graph.add_node(Node::Module {
            name: module.name.clone(),
            package_idx,
            file: module.file.clone(),
            definitions: module.definitions.clone(),
        });
        self.graph
            .add_edge(parent_idx, module_idx, EdgeWeight::Contains);
        self.module_map.insert(module.full_path.clone(), module_idx);

        self.stash_deps(&module.full_path, &module.dependencies);

        for child in &module.children {
            self.add_modules_recursive(child, package_idx, module_idx);
        }
    }

    fn add_package_deps(&mut self, packages: &[PackageInfo], include_tests: bool) {
        for package_info in packages {
            let Some(&from_idx) = self.package_map.get(&package_info.key) else {
                continue;
            };
            let prod = package_info.dependencies.iter().map(|dep| {
                (
                    dep,
                    EdgeWeight::PackageDep {
                        context: EdgeContext::production(),
                    },
                )
            });
            let dev = package_info.dev_dependencies.iter().map(|dep| {
                let edge = if include_tests {
                    EdgeWeight::PackageDep {
                        context: EdgeContext::test(TestKind::Unit),
                    }
                } else {
                    EdgeWeight::DevDep
                };
                (dep, edge)
            });
            prod.chain(dev)
                .filter_map(|(name, edge)| Some((self.package_map.get(name)?, edge)))
                .for_each(|(&to_idx, edge)| {
                    self.graph.add_edge(from_idx, to_idx, edge);
                });
        }
    }

    fn add_module_deps(&mut self) {
        // Clone to avoid borrow conflict (self.module_deps read vs self.resolve_node)
        let module_deps = std::mem::take(&mut self.module_deps);

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
                log_uses(from_path, &target, &target_deps);
                let locations = build_source_locations(&target_deps, &target);
                self.graph.add_edge(
                    from_idx,
                    to_idx,
                    EdgeWeight::ModuleDep { locations, context },
                );
            }
        }
    }

    fn add_externals(&mut self, ext: &ExternalsResult) {
        // Collect package IDs that are direct workspace dependencies for O(1) lookup.
        let direct_pkg_ids: HashSet<&str> = ext
            .workspace_deps
            .iter()
            .map(|dep| dep.external_pkg_id.as_str())
            .collect();

        // Add external package nodes, build package_id -> NodeIndex map
        let mut pkg_index: HashMap<&str, NodeIndex> = HashMap::new();
        for info in &ext.packages {
            let idx = self.graph.add_node(Node::ExternalPackage {
                name: info.name.clone(),
                version: info.version.clone(),
                package_id: info.package_id.clone(),
                is_direct_dependency: direct_pkg_ids.contains(info.package_id.as_str()),
                entry_files: info.entry_files.clone(),
                manifest: info.manifest.clone(),
            });
            pkg_index.insert(&info.package_id, idx);
            self.external_map.insert(info.name.clone(), idx);
        }

        // Workspace -> external PackageDep edges
        for dep in &ext.workspace_deps {
            let Some(&ext_idx) = pkg_index.get(dep.external_pkg_id.as_str()) else {
                continue;
            };
            let context = dep.context.clone();
            let Some(&ws_idx) = self.package_map.get(&dep.workspace_package) else {
                continue;
            };
            self.graph
                .add_edge(ws_idx, ext_idx, EdgeWeight::PackageDep { context });
        }

        // External -> external edges (only populated in transitive mode)
        for dep in &ext.external_deps {
            let from = pkg_index.get(dep.from_pkg_id.as_str());
            let to = pkg_index.get(dep.to_pkg_id.as_str());
            if let (Some(&from_idx), Some(&to_idx)) = (from, to) {
                let context = dep.context.clone();
                self.graph
                    .add_edge(from_idx, to_idx, EdgeWeight::PackageDep { context });
            }
        }
    }

    fn resolve_node(&self, name: &str) -> Option<NodeIndex> {
        self.module_map
            .get(name)
            .or_else(|| self.package_map.get(name))
            .or_else(|| self.external_map.get(name))
            .copied()
    }
}

/// List every use on one edge with its category, so the walker's result can
/// be read without a rule that consumes it. `STATIC_DATA` leaves the uses
/// out; the debug log is where they show.
fn log_uses(from: &str, target: &str, target_deps: &[&DependencyRef]) {
    for dep in target_deps {
        let Some(symbol) = &dep.target_item else {
            continue;
        };
        for symbol_use in &dep.uses {
            let inferred = if symbol_use.imported_for_methods {
                " (imported for its methods)"
            } else {
                ""
            };
            tracing::debug!(
                "use {from} -> {target}: {symbol} {}{inferred} at {}:{}",
                symbol_use.category,
                dep.source_file.display(),
                symbol_use.line
            );
        }
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
        .any(|dep| dep.context.kind == UsageKind::Production)
    {
        EdgeContext::production()
    } else {
        deps[0].context.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DefKind, Definition, DependencyRef, ModuleInfo, ModuleTree, PackageInfo};
    use crate::test_support::{conventional_package, module_node, package_node};
    use std::path::{Path, PathBuf};

    // -- Construction helpers --

    fn package(name: &str) -> PackageInfo {
        conventional_package(name, format!("/path/to/{name}"))
    }

    fn package_with_deps(name: &str, deps: &[&str]) -> PackageInfo {
        PackageInfo {
            dependencies: deps.iter().map(|&s| s.into()).collect(),
            ..package(name)
        }
    }

    fn module(name: &str, full_path: &str) -> ModuleInfo {
        ModuleInfo {
            name: name.into(),
            full_path: full_path.into(),
            file: None,
            children: vec![],
            dependencies: vec![],
            definitions: HashMap::new(),
        }
    }

    fn dep(target_package: &str, target_module: &str, file: &str, line: usize) -> DependencyRef {
        DependencyRef {
            target_package: target_package.into(),
            target_module: target_module.into(),
            target_item: None,
            source_file: file.into(),
            line,
            context: EdgeContext::production(),
            via_reexport: false,
            uses: Vec::new(),
            in_lib_view: false,
        }
    }

    fn reexport_dep(
        target_package: &str,
        target_module: &str,
        file: &str,
        line: usize,
    ) -> DependencyRef {
        DependencyRef {
            via_reexport: true,
            ..dep(target_package, target_module, file, line)
        }
    }

    fn tree(root: ModuleInfo) -> ModuleTree {
        ModuleTree {
            root,
            unparsed: Vec::new(),
        }
    }

    // -- Edge-query helpers --

    fn count_edges(graph: &ArcGraph) -> (usize, usize, usize) {
        graph.edge_indices().fold(
            (0, 0, 0),
            |(package_dep_count, module_dep_count, contains_count), edge_idx| match graph[edge_idx]
            {
                EdgeWeight::PackageDep { .. } => {
                    (package_dep_count + 1, module_dep_count, contains_count)
                }
                EdgeWeight::ModuleDep { .. } => {
                    (package_dep_count, module_dep_count + 1, contains_count)
                }
                EdgeWeight::Contains => (package_dep_count, module_dep_count, contains_count + 1),
                // Not a shown dependency; tests that care query it directly.
                EdgeWeight::DevDep => (package_dep_count, module_dep_count, contains_count),
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
                EdgeWeight::ModuleDep { context, locations } => {
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
    fn test_build_graph_single_package() {
        let graph = ArcGraph::build(&[package("my_crate")], &[], None, false);
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_build_graph_with_modules() {
        let packages = vec![package("my_crate")];
        let modules = vec![tree(ModuleInfo {
            children: vec![module("foo", "crate::foo"), module("bar", "crate::bar")],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&packages, &modules, None, false);
        assert_eq!(graph.node_count(), 3);
        let (cd, md, c) = count_edges(&graph);
        assert_eq!((cd, md, c), (0, 0, 2));
    }

    #[test]
    fn test_module_node_carries_the_module_file() {
        let modules = vec![tree(ModuleInfo {
            children: vec![ModuleInfo {
                file: Some("/ws/my_crate/src/foo.rs".into()),
                ..module("foo", "crate::foo")
            }],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&[package("my_crate")], &modules, None, false);
        let foo = graph
            .node_indices()
            .find(|&idx| graph[idx].name() == "foo")
            .expect("module node");
        assert!(matches!(
            &graph[foo],
            Node::Module { file: Some(file), .. } if file == Path::new("/ws/my_crate/src/foo.rs")
        ));
    }

    #[test]
    fn test_module_node_carries_the_module_definitions() {
        let widget = Definition {
            kind: DefKind::Struct,
            line: 7,
        };
        let modules = vec![tree(ModuleInfo {
            children: vec![ModuleInfo {
                definitions: HashMap::from([("Widget".to_string(), widget)]),
                ..module("foo", "crate::foo")
            }],
            ..module("my_crate", "crate")
        })];
        let graph = ArcGraph::build(&[package("my_crate")], &modules, None, false);
        let foo = graph
            .node_indices()
            .find(|&idx| graph[idx].name() == "foo")
            .expect("module node");
        assert!(matches!(
            &graph[foo],
            Node::Module { definitions, .. } if definitions.get("Widget") == Some(&widget)
        ));
    }

    #[test]
    fn test_build_graph_package_deps() {
        let packages = vec![
            package_with_deps("crate_a", &["crate_b"]),
            package("crate_b"),
        ];
        let graph = ArcGraph::build(&packages, &[], None, false);
        assert_eq!(graph.node_count(), 2);
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 1);
    }

    #[test]
    fn test_build_graph_package_deps_hyphenated_package_name() {
        let packages = vec![
            package_with_deps("crate-a", &["crate_b"]),
            package("crate-b"),
        ];
        let graph = ArcGraph::build(&packages, &[], None, false);
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 1);
    }

    /// The adapter decides how a name becomes a key; the graph only compares
    /// keys. `Shown` would never normalize to `shown_key`.
    #[test]
    fn packages_resolve_by_the_key_the_adapter_delivers() {
        let packages = vec![
            PackageInfo {
                key: "shown_key".into(),
                ..package("Shown")
            },
            package_with_deps("user", &["shown_key"]),
        ];
        let modules = vec![
            tree(ModuleInfo {
                children: vec![module("inner", "shown_key::inner")],
                ..module("shown_key", "shown_key")
            }),
            tree(ModuleInfo {
                dependencies: vec![dep("shown_key", "inner", "src/lib.rs", 1)],
                ..module("user", "user")
            }),
        ];
        let graph = ArcGraph::build(&packages, &modules, None, false);

        assert!(graph.node_weights().any(|node| node.name() == "Shown"));
        assert_eq!(count_edges(&graph), (1, 1, 1));
    }

    #[test]
    fn test_build_graph_module_deps() {
        let packages = vec![package("my_crate")];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);
        assert_eq!(graph.node_count(), 3);
        let (cd, md, c) = count_edges(&graph);
        assert_eq!((cd, md, c), (0, 1, 2));
    }

    #[test]
    fn test_build_graph_inter_package_module_deps() {
        let packages = vec![
            package_with_deps("crate_a", &["crate_b"]),
            package("crate_b"),
        ];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);
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
        let packages = vec![package("my_crate")];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);

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
        let packages = vec![package("crate_a")];
        let modules = vec![tree(ModuleInfo {
            children: vec![module("gamma", "crate_a::gamma")],
            dependencies: vec![dep("crate_a", "gamma", "src/lib.rs", 5)],
            ..module("crate_a", "crate_a")
        })];
        let graph = ArcGraph::build(&packages, &modules, None, false);
        let (_, locs) =
            find_module_dep(&graph, "crate_a", "gamma").expect("expected ModuleDep root→gamma");
        assert_eq!(locs[0].file, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn test_module_dep_to_package_node() {
        let packages = vec![
            package_with_deps("crate_a", &["crate_b"]),
            package("crate_b"),
        ];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);
        let (_, locs) = find_module_dep(&graph, "beta", "crate_b")
            .expect("expected ModuleDep from beta to crate_b");
        assert_eq!(locs[0].module_path, "crate_b");
        assert_eq!(locs[0].symbols, vec!["Widget"]);
    }

    #[test]
    fn test_root_dep_to_module() {
        let packages = vec![
            package_with_deps("crate_a", &["crate_b"]),
            package("crate_b"),
        ];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);
        let (_, locs) =
            find_module_dep(&graph, "crate_a", "gamma").expect("expected ModuleDep root→gamma");
        assert_eq!(locs[0].file, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn test_root_dep_to_package_node() {
        let packages = vec![
            package_with_deps("crate_a", &["crate_b"]),
            package("crate_b"),
        ];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);
        let (_, locs) = find_module_dep(&graph, "crate_a", "crate_b")
            .expect("expected ModuleDep crate_a→crate_b");
        assert_eq!(locs[0].module_path, "crate_b");
        assert_eq!(locs[0].symbols, vec!["Config"]);
    }

    #[test]
    fn test_cfg_test_dep_creates_test_edge() {
        let packages = vec![package("my_crate")];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);
        let (ctx, _) = find_module_dep(&graph, "bar", "foo").expect("expected ModuleDep bar→foo");
        assert_eq!(*ctx, EdgeContext::test(TestKind::Unit));
    }

    #[test]
    fn test_mixed_context_merges_into_production_edge() {
        let packages = vec![package("my_crate")];
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
        let graph = ArcGraph::build(&packages, &modules, None, false);
        let (ctx, locs) =
            find_module_dep(&graph, "bar", "foo").expect("expected ModuleDep bar→foo");
        assert_eq!(*ctx, EdgeContext::production());
        assert_eq!(locs.len(), 2);
    }

    #[test]
    fn test_external_package_node_properties() {
        let node = Node::ExternalPackage {
            name: "serde".into(),
            version: "1.0.0".into(),
            package_id: "serde 1.0.0 (registry+...)".into(),
            is_direct_dependency: true,
            entry_files: vec![],
            manifest: Some("/reg/serde/Cargo.toml".into()),
        };
        assert!(!node.is_package());
        assert!(node.is_external());
        assert_eq!(node.name(), "serde");
    }

    #[test]
    fn test_production_reachable_excludes_external() {
        let mut graph = ArcGraph::new();
        let package_idx = graph.add_node(package_node("my_crate"));
        let mod_idx = graph.add_node(module_node("foo", package_idx));
        graph.add_edge(package_idx, mod_idx, EdgeWeight::Contains);
        let ext_idx = graph.add_node(Node::ExternalPackage {
            name: "serde".into(),
            version: "1.0.0".into(),
            package_id: "serde-pkg".into(),
            is_direct_dependency: true,
            entry_files: vec![],
            manifest: Some("/reg/serde/Cargo.toml".into()),
        });
        graph.add_edge(
            package_idx,
            ext_idx,
            EdgeWeight::PackageDep {
                context: EdgeContext::production(),
            },
        );
        let reachable = graph.production_reachable();
        assert!(reachable.contains(&package_idx));
        assert!(
            !reachable.contains(&ext_idx),
            "ExternalPackage should not be in production_reachable"
        );
    }

    #[test]
    fn test_production_reachable_packages_without_submodules() {
        let mut graph = ArcGraph::new();
        let a = graph.add_node(package_node("alpha"));
        let b = graph.add_node(package_node("beta"));
        graph.add_edge(
            a,
            b,
            EdgeWeight::PackageDep {
                context: EdgeContext::production(),
            },
        );
        // No Contains edges anywhere → pure package-level diagram
        let reachable = graph.production_reachable();
        assert!(reachable.contains(&a), "alpha should be reachable");
        assert!(reachable.contains(&b), "beta should be reachable");
    }

    /// Package with modules, a leaf binary depending on it, and a test helper the
    /// binary only pulls in for tests. The helper depends on production code
    /// itself, so only the incoming edges tell the two module-less packages apart.
    fn mixed_graph_with_leaf_and_helper() -> (ArcGraph, NodeIndex, NodeIndex) {
        let mut graph = ArcGraph::new();
        let lib = graph.add_node(package_node("lib"));
        let module = graph.add_node(module_node("engine", lib));
        graph.add_edge(lib, module, EdgeWeight::Contains);

        let binary = graph.add_node(package_node("binary"));
        let helper = graph.add_node(package_node("helper"));
        for source in [binary, helper] {
            graph.add_edge(
                source,
                lib,
                EdgeWeight::PackageDep {
                    context: EdgeContext::production(),
                },
            );
        }
        graph.add_edge(binary, helper, EdgeWeight::DevDep);
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
    fn test_production_reachable_drops_dev_only_package() {
        let (graph, _, helper) = mixed_graph_with_leaf_and_helper();
        assert!(
            !graph.production_reachable().contains(&helper),
            "a package only reached through a dev-dependency is test infrastructure"
        );
    }

    #[test]
    fn test_owning_package_external() {
        let mut graph = ArcGraph::new();
        let ext_idx = graph.add_node(Node::ExternalPackage {
            name: "serde".into(),
            version: "1.0.0".into(),
            package_id: "serde-pkg".into(),
            is_direct_dependency: true,
            entry_files: vec![],
            manifest: Some("/reg/serde/Cargo.toml".into()),
        });
        assert_eq!(graph.owning_package(ext_idx), ext_idx);
    }

    #[test]
    fn test_qualified_name_disambiguates_same_leaf() {
        let mut graph = ArcGraph::new();
        let package_idx = graph.add_node(package_node("my-crate"));
        let top_store = graph.add_node(module_node("store", package_idx));
        graph.add_edge(package_idx, top_store, EdgeWeight::Contains);
        let core = graph.add_node(module_node("core", package_idx));
        graph.add_edge(package_idx, core, EdgeWeight::Contains);
        let core_store = graph.add_node(module_node("store", package_idx));
        graph.add_edge(core, core_store, EdgeWeight::Contains);

        assert_eq!(graph.qualified_name(top_store), "my-crate::store");
        assert_eq!(graph.qualified_name(core_store), "my-crate::core::store");
    }

    #[test]
    fn test_qualified_name_orphan_falls_back_to_leaf() {
        let mut graph = ArcGraph::new();
        let idx = graph.add_node(module_node("lonely", NodeIndex::new(0)));
        assert_eq!(graph.qualified_name(idx), "lonely");
    }

    #[test]
    fn test_qualified_name_package_returns_bare_name() {
        let mut graph = ArcGraph::new();
        let package_idx = graph.add_node(package_node("my-crate"));
        assert_eq!(graph.qualified_name(package_idx), "my-crate");
    }

    #[test]
    fn test_build_graph_with_externals() {
        use crate::model::{ExternalDep, ExternalPackageInfo, WorkspaceExternalDep};

        let packages = vec![package("my_crate")];
        let externals = ExternalsResult {
            packages: vec![
                ExternalPackageInfo {
                    name: "serde".into(),
                    version: "1.0.0".into(),
                    package_id: "serde-pkg".into(),
                    entry_files: vec![],
                    manifest: Some("/reg/serde/Cargo.toml".into()),
                },
                ExternalPackageInfo {
                    name: "tokio".into(),
                    version: "1.0.0".into(),
                    package_id: "tokio-pkg".into(),
                    entry_files: vec![],
                    manifest: Some("/reg/tokio/Cargo.toml".into()),
                },
            ],
            workspace_deps: vec![WorkspaceExternalDep {
                workspace_package: "my_crate".into(),
                external_pkg_id: "serde-pkg".into(),
                context: EdgeContext::production(),
            }],
            external_deps: vec![ExternalDep {
                from_pkg_id: "serde-pkg".into(),
                to_pkg_id: "tokio-pkg".into(),
                context: EdgeContext::production(),
            }],
        };
        let graph = ArcGraph::build(&packages, &[], Some(&externals), false);
        // 1 workspace + 2 external = 3 nodes
        assert_eq!(graph.node_count(), 3);
        // 1 workspace->serde + 1 serde->tokio = 2 PackageDep edges
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 2);
    }

    #[test]
    fn test_package_nodes_carry_their_entry_files() {
        use crate::model::ExternalPackageInfo;

        let app_entries = vec![
            EntryFile::lib("/ws/app/src/lib.rs"),
            EntryFile::bin("/ws/app/src/main.rs"),
        ];
        let serde_entries = vec![EntryFile::lib("/reg/serde-1.0.0/src/lib.rs")];
        let packages = vec![PackageInfo {
            entry_files: app_entries.clone(),
            manifest: Some("/ws/app/Cargo.toml".into()),
            ..package("app")
        }];
        let externals = ExternalsResult {
            packages: vec![ExternalPackageInfo {
                name: "serde".into(),
                version: "1.0.0".into(),
                package_id: "serde-pkg".into(),
                entry_files: serde_entries.clone(),
                manifest: Some("/reg/serde-1.0.0/Cargo.toml".into()),
            }],
            workspace_deps: vec![],
            external_deps: vec![],
        };
        let graph = ArcGraph::build(&packages, &[], Some(&externals), false);

        let node = |name: &str| {
            let idx = graph
                .node_indices()
                .find(|&idx| graph[idx].name() == name)
                .expect("node");
            &graph[idx]
        };
        assert!(matches!(
            node("app"),
            Node::Package { entry_files, manifest, .. }
                if *entry_files == app_entries
                    && manifest.as_deref() == Some(Path::new("/ws/app/Cargo.toml"))
        ));
        assert!(matches!(
            node("serde"),
            Node::ExternalPackage { entry_files, manifest, .. }
                if *entry_files == serde_entries
                    && manifest.as_deref() == Some(Path::new("/reg/serde-1.0.0/Cargo.toml"))
        ));
    }

    #[test]
    fn test_external_is_direct_dependency_flag() {
        use crate::model::{ExternalDep, ExternalPackageInfo, WorkspaceExternalDep};

        let packages = vec![package("my_crate")];
        let externals = ExternalsResult {
            packages: vec![
                ExternalPackageInfo {
                    name: "serde".into(),
                    version: "1.0.0".into(),
                    package_id: "serde-pkg".into(),
                    entry_files: vec![],
                    manifest: Some("/reg/serde/Cargo.toml".into()),
                },
                ExternalPackageInfo {
                    name: "tokio".into(),
                    version: "1.0.0".into(),
                    package_id: "tokio-pkg".into(),
                    entry_files: vec![],
                    manifest: Some("/reg/tokio/Cargo.toml".into()),
                },
            ],
            workspace_deps: vec![WorkspaceExternalDep {
                workspace_package: "my_crate".into(),
                external_pkg_id: "serde-pkg".into(),
                context: EdgeContext::production(),
            }],
            external_deps: vec![ExternalDep {
                from_pkg_id: "serde-pkg".into(),
                to_pkg_id: "tokio-pkg".into(),
                context: EdgeContext::production(),
            }],
        };
        let graph = ArcGraph::build(&packages, &[], Some(&externals), false);

        // serde is a direct workspace dependency
        let serde = graph
            .node_indices()
            .find(|&idx| graph[idx].name() == "serde")
            .expect("serde node should exist");
        assert!(
            matches!(
                &graph[serde],
                Node::ExternalPackage {
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
                Node::ExternalPackage {
                    is_direct_dependency: false,
                    ..
                }
            ),
            "tokio should be a transitive dependency"
        );
    }

    #[test]
    fn test_build_graph_externals_none() {
        let packages = vec![package_with_deps("a", &["b"]), package("b")];
        let graph = ArcGraph::build(&packages, &[], None, false);
        assert_eq!(graph.node_count(), 2);
        let (cd, _, _) = count_edges(&graph);
        assert_eq!(cd, 1);
    }

    #[test]
    fn test_resolve_node_finds_external() {
        use crate::model::ExternalPackageInfo;

        let packages = vec![package("my_crate")];
        let externals = ExternalsResult {
            packages: vec![ExternalPackageInfo {
                name: "serde".into(),
                version: "1.0.0".into(),
                package_id: "serde-pkg".into(),
                entry_files: vec![],
                manifest: Some("/reg/serde/Cargo.toml".into()),
            }],
            workspace_deps: vec![],
            external_deps: vec![],
        };
        let graph = ArcGraph::build(&packages, &[], Some(&externals), false);
        // Verify the external node exists
        let ext_node = graph
            .node_indices()
            .find(|&idx| graph[idx].name() == "serde");
        assert!(ext_node.is_some(), "should find serde external node");
        assert!(graph[ext_node.unwrap()].is_external());
    }
}
