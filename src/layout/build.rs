//! Build layout IR from graph and cycle information.

use super::cycles::Cycle;
use super::toposort::stable_toposort;
use crate::graph::{ArcGraph, Edge, Node};
use crate::model::{EdgeContext, SourceLocation};
use crate::volatility::Volatility;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

/// Index into LayoutIR.items
pub type NodeId = usize;

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Crate,
    Module { nesting: u32, parent: NodeId },
}

#[derive(Debug, Clone)]
pub struct LayoutItem {
    pub id: NodeId,
    pub kind: ItemKind,
    pub label: String,
    pub source_path: Option<String>,
    pub volatility: Option<(Volatility, usize)>,
}

impl LayoutItem {
    pub fn new(id: NodeId, kind: ItemKind, label: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            label: label.into(),
            source_path: None,
            volatility: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeDirection {
    Downward,
    Upward,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CycleKind {
    Direct,
    Transitive,
}

#[derive(Debug, Clone)]
pub struct LayoutEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub direction: EdgeDirection,
    pub cycle: Option<CycleKind>,
    pub cycle_ids: Vec<usize>,
    pub source_locations: Vec<SourceLocation>,
    pub context: EdgeContext,
}

impl LayoutEdge {
    /// Create a new edge with auto-computed direction.
    /// `Downward` when `from < to`, `Upward` otherwise.
    pub fn new(from: NodeId, to: NodeId, context: EdgeContext) -> Self {
        let direction = if from < to {
            EdgeDirection::Downward
        } else {
            EdgeDirection::Upward
        };
        Self {
            from,
            to,
            direction,
            cycle: None,
            cycle_ids: vec![],
            source_locations: vec![],
            context,
        }
    }

    pub fn with_cycle(mut self, kind: CycleKind, ids: Vec<usize>) -> Self {
        self.cycle = Some(kind);
        self.cycle_ids = ids;
        self
    }

    pub fn with_source_locations(mut self, locations: Vec<SourceLocation>) -> Self {
        self.source_locations = locations;
        self
    }
}

#[derive(Debug, Default)]
pub struct LayoutIR {
    pub items: Vec<LayoutItem>,
    pub edges: Vec<LayoutEdge>,
}

impl LayoutIR {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_item(&mut self, kind: ItemKind, label: String) -> NodeId {
        let id = self.items.len();
        self.items.push(LayoutItem::new(id, kind, label));
        id
    }
}

/// Build LayoutIR from graph and cycle information.
/// Converts graph nodes to LayoutItems with proper nesting and edges with cycle markers.
/// CrateDep edges are skipped when ModuleDep edges exist between the same crates.
pub fn build_layout(graph: &ArcGraph, cycles: &[Cycle]) -> LayoutIR {
    let mut ir = LayoutIR::new();
    let edge_to_cycles = build_edge_cycle_index(cycles);
    let parent_map = graph.parent_map();

    let (crate_indices, module_indices): (Vec<_>, Vec<_>) =
        graph.node_indices().partition(|&idx| graph[idx].is_crate());
    let crate_indices = graph.order_crates(&crate_indices);
    let reachable = graph.production_reachable();
    let ordered = graph.order_items(&crate_indices, &module_indices, &reachable);
    let node_map = populate_items(&mut ir, graph, &ordered, &parent_map);

    let suppressed = graph.suppressed_crate_pairs();
    populate_edges(&mut ir, graph, &node_map, &edge_to_cycles, &suppressed);

    ir
}

/// Convert graph nodes to LayoutItems, returning the NodeIndex → NodeId map.
fn populate_items(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    ordered: &[NodeIndex],
    parent_map: &HashMap<NodeIndex, NodeIndex>,
) -> HashMap<NodeIndex, NodeId> {
    let mut node_map = HashMap::new();
    for &idx in ordered {
        let (kind, label, source_path) = match &graph[idx] {
            Node::Crate { name, .. } => (ItemKind::Crate, name.clone(), None),
            Node::Module { name, .. } => {
                let nesting = nesting_depth(idx, parent_map);
                let parent_layout_id = parent_map
                    .get(&idx)
                    .and_then(|&p| node_map.get(&p))
                    .copied()
                    .unwrap_or(0);
                (
                    ItemKind::Module {
                        nesting,
                        parent: parent_layout_id,
                    },
                    name.clone(),
                    module_source_path(graph, idx),
                )
            }
        };

        let layout_id = ir.add_item(kind, label);
        node_map.insert(idx, layout_id);
        ir.items[layout_id].source_path = source_path;
    }
    node_map
}

/// Add dependency edges (CrateDep and ModuleDep) to the layout IR.
/// CrateDep and ModuleDep arms are unified — they share direction + cycle computation
/// and only differ in suppression check and source locations.
fn populate_edges(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    node_map: &HashMap<NodeIndex, NodeId>,
    edge_to_cycles: &HashMap<(NodeIndex, NodeIndex), Vec<usize>>,
    suppressed: &HashSet<(NodeIndex, NodeIndex)>,
) {
    for edge in graph.edge_references() {
        let (src, dst) = (edge.source(), edge.target());

        let (locations, context, is_module_dep) = match edge.weight() {
            Edge::CrateDep { context } => {
                if suppressed.contains(&(src, dst)) {
                    continue;
                }
                (vec![], context.clone(), false)
            }
            Edge::ModuleDep { locations, context } => (locations.clone(), context.clone(), true),
            Edge::Contains => continue,
        };

        if let (Some(&from), Some(&to)) = (node_map.get(&src), node_map.get(&dst)) {
            let (cycle, cycle_ids) =
                compute_cycle_info(src, dst, edge_to_cycles, graph, is_module_dep);
            ir.edges.push(LayoutEdge {
                cycle,
                cycle_ids,
                source_locations: locations,
                ..LayoutEdge::new(from, to, context)
            });
        }
    }
}

/// Find the source file path for a module by inspecting its outgoing ModuleDep edges.
fn module_source_path(graph: &ArcGraph, idx: NodeIndex) -> Option<String> {
    graph
        .edges_directed(idx, petgraph::Direction::Outgoing)
        .find_map(|edge| match edge.weight() {
            Edge::ModuleDep { locations, .. } => {
                locations.first().map(|loc| loc.file.display().to_string())
            }
            _ => None,
        })
}

/// Calculate nesting depth for a node by walking up the parent chain.
fn nesting_depth(idx: NodeIndex, parent_map: &HashMap<NodeIndex, NodeIndex>) -> u32 {
    let mut depth = 0u32;
    let mut current = idx;
    while let Some(&parent) = parent_map.get(&current) {
        depth += 1;
        current = parent;
    }
    depth
}

/// Build edge-to-cycles mapping: for each directed edge (src, dst) in any cycle,
/// record which cycle indices it belongs to.
fn build_edge_cycle_index(cycles: &[Cycle]) -> HashMap<(NodeIndex, NodeIndex), Vec<usize>> {
    let mut edge_to_cycles: HashMap<(NodeIndex, NodeIndex), Vec<usize>> = HashMap::new();
    for (cycle_idx, cycle) in cycles.iter().enumerate() {
        for (src, dst) in cycle.edges() {
            edge_to_cycles
                .entry((src, dst))
                .or_default()
                .push(cycle_idx);
        }
    }
    edge_to_cycles
}

/// Determine cycle kind and cycle IDs for an edge.
/// CrateDep edges are always `Transitive` when in a cycle.
/// ModuleDep edges check for a back-edge to distinguish `Direct` vs `Transitive`.
fn compute_cycle_info(
    src: NodeIndex,
    dst: NodeIndex,
    edge_to_cycles: &HashMap<(NodeIndex, NodeIndex), Vec<usize>>,
    graph: &ArcGraph,
    is_module_dep: bool,
) -> (Option<CycleKind>, Vec<usize>) {
    let cycle_ids = edge_to_cycles.get(&(src, dst)).cloned().unwrap_or_default();
    if cycle_ids.is_empty() {
        return (None, cycle_ids);
    }

    let has_reverse_module_dep = is_module_dep
        && graph
            .find_edge(dst, src)
            .is_some_and(|ei| matches!(graph[ei], Edge::ModuleDep { .. }));

    let kind = if has_reverse_module_dep {
        CycleKind::Direct
    } else {
        CycleKind::Transitive
    };
    (Some(kind), cycle_ids)
}

/// Extension trait for incrementing weighted edge counts.
trait IncrementEdge {
    fn increment_edge(&mut self, src: NodeIndex, dst: NodeIndex);
}

impl<N> IncrementEdge for DiGraph<N, usize> {
    fn increment_edge(&mut self, src: NodeIndex, dst: NodeIndex) {
        if let Some(ei) = self.find_edge(src, dst) {
            self[ei] += 1;
        } else {
            self.add_edge(src, dst, 1);
        }
    }
}

/// Build a mini dependency graph among sibling nodes based on cross-subtree dependencies.
/// For each sibling, collects its full subtree and counts how many production ModuleDep
/// edges cross from one sibling's subtree to another's.
fn build_sibling_dep_graph(children: &[NodeIndex], graph: &ArcGraph) -> DiGraph<NodeIndex, usize> {
    // Collect subtrees for each sibling (child + all descendants)
    let subtrees: HashMap<NodeIndex, HashSet<NodeIndex>> = children
        .iter()
        .map(|&child| (child, graph.containment_subtree(child)))
        .collect();

    // Build mini graph
    let mut sibling_deps: DiGraph<NodeIndex, usize> = DiGraph::new();
    let orig_to_local: HashMap<NodeIndex, petgraph::graph::NodeIndex> = children
        .iter()
        .map(|&child| (child, sibling_deps.add_node(child)))
        .collect();

    // Inverted index: for each node, which sibling's subtree contains it?
    let node_to_sibling: HashMap<NodeIndex, NodeIndex> = subtrees
        .iter()
        .flat_map(|(&child, subtree)| subtree.iter().map(move |&n| (n, child)))
        .collect();

    // Cross-subtree dependencies
    let node_to_sibling = &node_to_sibling;
    let cross_subtree_deps = children.iter().flat_map(|&child| {
        subtrees[&child]
            .iter()
            .flat_map(move |&node| graph.edges(node))
            .filter(|e| e.weight().is_production_module_dep())
            .filter_map(move |e| {
                let &sibling = node_to_sibling.get(&e.target())?;
                (sibling != child).then_some((child, sibling))
            })
    });

    for (child, sibling) in cross_subtree_deps {
        sibling_deps.increment_edge(orig_to_local[&child], orig_to_local[&sibling]);
    }

    sibling_deps
}

/// Layout-specific methods on `ArcGraph`.
/// Kept in `build.rs` (not `graph.rs`) because they depend on `build_sibling_dep_graph`
/// and `stable_toposort`, which are layout-specific.
impl ArcGraph {
    /// Hierarchically sorted modules for a parent, collecting children recursively.
    /// Children are sorted topologically by ModuleDep edges, with alphabetical tie-breaker.
    /// Also considers cross-subtree dependencies: if any node in subtree(A) depends on
    /// any node in subtree(B), then A should appear before B.
    fn ordered_children(
        &self,
        parent: NodeIndex,
        module_indices: &[NodeIndex],
        added: &mut HashSet<NodeIndex>,
    ) -> Vec<NodeIndex> {
        // Find direct children of this parent (via Contains edge)
        let mut children: Vec<NodeIndex> = module_indices
            .iter()
            .filter(|&&m| !added.contains(&m) && self.contains_child(parent, m))
            .copied()
            .collect();

        // FIRST: Sort alphabetically (provides stable base order for toposort)
        children.sort_unstable_by(|&a, &b| self[a].name().cmp(self[b].name()));

        let sibling_deps = build_sibling_dep_graph(&children, self);

        // THEN: Stable topological sort using Kahn's algorithm
        // This preserves alphabetical order for independent nodes (tie-breaker)
        let sorted = stable_toposort(&sibling_deps, |idx| self[idx].name().to_owned());
        if !sorted.is_empty() {
            children = sorted;
        }
        // On cycles (empty result): keep alphabetical order

        // Add each child + its descendants recursively
        children
            .into_iter()
            .flat_map(|child| {
                added.insert(child);
                std::iter::once(child).chain(self.ordered_children(child, module_indices, added))
            })
            .collect()
    }

    /// Re-sort crates by aggregated inter-crate dependencies (CrateDep + ModuleDep).
    /// Builds a crate-level dependency graph and runs stable toposort with alphabetical tie-breaking.
    fn order_crates(&self, crate_indices: &[NodeIndex]) -> Vec<NodeIndex> {
        let mut crate_graph: DiGraph<NodeIndex, usize> = DiGraph::new();
        let mut sorted_crates = crate_indices.to_vec();
        sorted_crates.sort_unstable_by_key(|n| n.index());
        let orig_to_local: HashMap<NodeIndex, petgraph::graph::NodeIndex> = sorted_crates
            .iter()
            .map(|&ci| (ci, crate_graph.add_node(ci)))
            .collect();

        for edge in self.edge_references() {
            if edge.weight().is_production() {
                let src_crate = self.owning_crate(edge.source());
                let dst_crate = self.owning_crate(edge.target());
                if src_crate != dst_crate
                    && let (Some(&sc), Some(&dc)) =
                        (orig_to_local.get(&src_crate), orig_to_local.get(&dst_crate))
                {
                    crate_graph.increment_edge(sc, dc);
                }
            }
        }

        stable_toposort(&crate_graph, |idx| self[idx].name().to_owned())
    }

    /// Find crate pairs where ModuleDep edges exist (so CrateDep can be suppressed).
    /// Entry-point imports create ModuleDep edges where one or both endpoints
    /// are Node::Crate (not just Node::Module), so we handle all combinations.
    fn suppressed_crate_pairs(&self) -> HashSet<(NodeIndex, NodeIndex)> {
        self.edge_references()
            .filter_map(|edge| match edge.weight() {
                Edge::ModuleDep { .. } => {
                    let src_crate = self.owning_crate(edge.source());
                    let dst_crate = self.owning_crate(edge.target());
                    (src_crate != dst_crate).then_some((src_crate, dst_crate))
                }
                _ => None,
            })
            .collect()
    }

    /// Group modules under reachable crates and collect orphans.
    fn order_items(
        &self,
        crate_indices: &[NodeIndex],
        module_indices: &[NodeIndex],
        reachable: &HashSet<NodeIndex>,
    ) -> Vec<NodeIndex> {
        let mut ordered = Vec::new();
        let mut added = HashSet::new();
        for &ci in crate_indices {
            if !reachable.contains(&ci) {
                continue;
            }
            ordered.push(ci);
            ordered.extend(self.ordered_children(ci, module_indices, &mut added));
        }
        // Orphans: modules not claimed by any reachable crate
        for &mi in module_indices {
            if !added.contains(&mi) {
                ordered.push(mi);
            }
        }
        ordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ArcGraph, Edge, Node};
    use crate::layout::ElementaryCycles;
    use crate::model::{DependencyKind, EdgeContext, SourceLocation, TestKind};
    use assert2::check;
    use petgraph::graph::NodeIndex;
    use rstest::rstest;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    struct TestGraphBuilder {
        graph: ArcGraph,
        names: HashMap<String, NodeIndex>,
    }

    impl TestGraphBuilder {
        fn new() -> Self {
            Self {
                graph: ArcGraph::new(),
                names: HashMap::new(),
            }
        }

        /// Add a crate with child modules and Contains edges.
        /// Path is auto-generated as "/<crate_name>".
        fn crate_with_modules(&mut self, crate_name: &str, module_names: &[&str]) -> &mut Self {
            let crate_idx = self.graph.add_node(Node::Crate {
                name: crate_name.to_string(),
                path: PathBuf::from(format!("/{crate_name}")),
            });
            self.names.insert(crate_name.to_string(), crate_idx);
            for &mod_name in module_names {
                let mod_idx = self.graph.add_node(Node::Module {
                    name: mod_name.to_string(),
                    crate_idx,
                });
                self.names.insert(mod_name.to_string(), mod_idx);
                self.graph.add_edge(crate_idx, mod_idx, Edge::Contains);
            }
            self
        }

        /// Add a module not attached to any crate (uses NodeIndex::new(0) as crate_idx).
        fn orphan_module(&mut self, name: &str) -> &mut Self {
            let idx = self.graph.add_node(Node::Module {
                name: name.to_string(),
                crate_idx: NodeIndex::new(0),
            });
            self.names.insert(name.to_string(), idx);
            self
        }

        /// Add a nested module under an existing parent (module or crate), with Contains edge.
        fn nested_module(&mut self, parent: &str, child: &str) -> &mut Self {
            let parent_idx = self.names[parent];
            let crate_idx = match &self.graph[parent_idx] {
                Node::Module { crate_idx, .. } => *crate_idx,
                Node::Crate { .. } => parent_idx,
            };
            let child_idx = self.graph.add_node(Node::Module {
                name: child.to_string(),
                crate_idx,
            });
            self.names.insert(child.to_string(), child_idx);
            self.graph.add_edge(parent_idx, child_idx, Edge::Contains);
            self
        }

        /// Add a production ModuleDep edge (empty locations).
        fn prod_dep(&mut self, from: &str, to: &str) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                Edge::ModuleDep {
                    locations: vec![],
                    context: EdgeContext::production(),
                },
            );
            self
        }

        /// Add a test ModuleDep edge (empty locations).
        fn test_dep(&mut self, from: &str, to: &str, kind: TestKind) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                Edge::ModuleDep {
                    locations: vec![],
                    context: EdgeContext::test(kind),
                },
            );
            self
        }

        /// Add a production CrateDep edge.
        fn crate_dep(&mut self, from: &str, to: &str) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                Edge::CrateDep {
                    context: EdgeContext::production(),
                },
            );
            self
        }

        /// Add a test CrateDep edge.
        fn test_crate_dep(&mut self, from: &str, to: &str, kind: TestKind) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                Edge::CrateDep {
                    context: EdgeContext::test(kind),
                },
            );
            self
        }

        /// Add a ModuleDep with a source location.
        fn prod_dep_with_location(
            &mut self,
            from: &str,
            to: &str,
            file: &str,
            line: usize,
            symbols: &[&str],
            module_path: &str,
        ) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                Edge::ModuleDep {
                    locations: vec![SourceLocation {
                        file: PathBuf::from(file),
                        line,
                        symbols: symbols.iter().map(|s| s.to_string()).collect(),
                        module_path: module_path.to_string(),
                    }],
                    context: EdgeContext::production(),
                },
            );
            self
        }

        /// Add a ModuleDep with a dummy source location (for suppression tests).
        fn prod_dep_located(&mut self, from: &str, to: &str) -> &mut Self {
            self.prod_dep_with_location(from, to, "src/dummy.rs", 1, &["Sym"], to)
        }

        /// Consume and return (graph, name->NodeIndex map).
        fn build(self) -> (ArcGraph, HashMap<String, NodeIndex>) {
            (self.graph, self.names)
        }
    }

    /// Thin wrapper around `LayoutIR` to eliminate repetitive label-extraction boilerplate.
    struct LayoutAssert {
        ir: LayoutIR,
    }

    impl LayoutAssert {
        fn new(ir: LayoutIR) -> Self {
            Self { ir }
        }

        fn labels(&self) -> Vec<&str> {
            self.ir.items.iter().map(|i| i.label.as_str()).collect()
        }

        fn pos(&self, name: &str) -> usize {
            self.ir
                .items
                .iter()
                .position(|i| i.label == name)
                .unwrap_or_else(|| panic!("label {name:?} not found in {:?}", self.labels()))
        }

        fn assert_order(&self, before: &str, after: &str) {
            assert!(
                self.pos(before) < self.pos(after),
                "{before:?} should come before {after:?}. Labels: {:?}",
                self.labels()
            );
        }

        /// Assert that the given labels appear in exactly this order (ignoring other items).
        fn assert_top_level_order(&self, expected: &[&str]) {
            let expected_set: HashSet<&str> = expected.iter().copied().collect();
            let actual: Vec<&str> = self
                .labels()
                .into_iter()
                .filter(|l| expected_set.contains(l))
                .collect();
            assert_eq!(
                actual,
                expected,
                "Top-level order mismatch. All labels: {:?}",
                self.labels()
            );
        }
    }

    #[test]
    fn test_layout_edge_carries_edge_context() {
        let prod_edge = LayoutEdge::new(0, 1, EdgeContext::production());
        assert_eq!(prod_edge.context.kind, DependencyKind::Production);

        let test_edge = LayoutEdge::new(0, 1, EdgeContext::test(TestKind::Unit));
        assert_eq!(test_edge.context.kind, DependencyKind::Test(TestKind::Unit));
    }

    #[test]
    fn test_layout_edge_has_source_locations() {
        let edge = LayoutEdge::new(0, 1, EdgeContext::production()).with_source_locations(vec![
            SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 42,
                symbols: vec![],
                module_path: String::new(),
            },
        ]);
        assert_eq!(edge.source_locations.len(), 1);
        assert_eq!(edge.source_locations[0].line, 42);
    }

    // === Build Layout Tests ===

    #[test]
    fn test_build_layout_single_crate() {
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("my_crate", &["mod_a", "mod_b"])
            .prod_dep("mod_a", "mod_b");
        let (graph, _) = b.build();
        let ir = build_layout(&graph, &[]);

        // Should have 3 items (1 crate + 2 modules)
        assert_eq!(ir.items.len(), 3);

        // Should have 1 dependency edge (mod_a -> mod_b)
        assert_eq!(ir.edges.len(), 1);
        assert_eq!(ir.edges[0].direction, EdgeDirection::Downward);
        assert!(ir.edges[0].cycle.is_none());
    }

    #[test]
    fn test_build_layout_with_cycles() {
        let mut b = TestGraphBuilder::new();
        b.orphan_module("a")
            .orphan_module("b")
            .prod_dep("a", "b")
            .prod_dep("b", "a");
        let (graph, _) = b.build();
        let cycles = graph.production_subgraph().elementary_cycles();
        let ir = build_layout(&graph, &cycles);

        // Should have 2 items
        assert_eq!(ir.items.len(), 2);

        // Should have 2 edges, both marked as cycle edges
        assert_eq!(ir.edges.len(), 2);
        for edge in &ir.edges {
            assert!(edge.cycle.is_some(), "Cycle edges should be marked");
        }
    }

    #[test]
    fn test_cycle_ids_propagation() {
        // Build graph: crate with 6 modules, two independent cycles
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test", &["a", "b", "c", "d", "e", "f"])
            // Cycle 1: A → B → C → A
            .prod_dep("a", "b")
            .prod_dep("b", "c")
            .prod_dep("c", "a")
            // Cycle 2: D → E → D
            .prod_dep("d", "e")
            .prod_dep("e", "d")
            // Non-cycle edge: F → A
            .prod_dep("f", "a");
        let (graph, _) = b.build();
        let cycles = graph.production_subgraph().elementary_cycles();
        let ir = build_layout(&graph, &cycles);

        // Should have at least 5 cycle edges (3 from cycle 1 + 2 from cycle 2)
        let cycle_edges: Vec<_> = ir.edges.iter().filter(|e| e.cycle.is_some()).collect();
        check!(cycle_edges.len() >= 5);

        // All cycle edges should have non-empty cycle_ids
        for edge in &cycle_edges {
            check!(!edge.cycle_ids.is_empty());
        }

        // Should have exactly 2 distinct cycle IDs
        let all_ids: HashSet<usize> = cycle_edges
            .iter()
            .flat_map(|e| e.cycle_ids.iter().copied())
            .collect();
        check!(all_ids.len() == 2);

        // Non-cycle edges (F → A) should have empty cycle_ids
        let non_cycle_edges: Vec<_> = ir.edges.iter().filter(|e| e.cycle.is_none()).collect();
        for edge in &non_cycle_edges {
            check!(edge.cycle_ids.is_empty());
        }
    }

    #[test]
    fn test_upward_edge_direction() {
        // When a module that appears later in topo order depends on one that appears earlier,
        // it should be marked as Downward. When the reverse happens (earlier depends on later),
        // it should be marked as Upward.
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test", &["a", "b"]).prod_dep("a", "b"); // a depends on b
        let (graph, _) = b.build();
        let ir = build_layout(&graph, &[]);

        // There should be exactly one edge
        assert_eq!(ir.edges.len(), 1);
        let edge = &ir.edges[0];

        // The direction depends on topo order position
        // If from < to in layout order -> Downward
        // If from > to in layout order -> Upward
        assert!(
            edge.cycle.is_none(),
            "Edge should not be a cycle: {:?}",
            edge.cycle
        );
    }

    #[test]
    fn test_build_layout_multi_crate_grouping() {
        // Simulate a workspace with 2 crates, each having 2 modules
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("crate_a", &["mod_a1", "mod_a2"])
            .crate_with_modules("crate_b", &["mod_b1", "mod_b2"])
            .crate_dep("crate_a", "crate_b");
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        check!(la.ir.items.len() == 6);

        // Crate A's modules between crate_a and crate_b
        check!(la.pos("crate_a") < la.pos("mod_a1"));
        check!(la.pos("mod_a1") < la.pos("crate_b"));
        check!(la.pos("crate_a") < la.pos("mod_a2"));
        check!(la.pos("mod_a2") < la.pos("crate_b"));

        // Crate B's modules after crate_b
        check!(la.pos("crate_b") < la.pos("mod_b1"));
        check!(la.pos("crate_b") < la.pos("mod_b2"));

        // CrateDep edge should be present (no ModuleDeps between crates)
        check!(la.ir.edges.len() == 1);
        check!(la.ir.edges[0].from == la.pos("crate_a"));
        check!(la.ir.edges[0].to == la.pos("crate_b"));
    }

    // === Layout Item Tests ===

    #[test]
    fn test_layout_item_creation() {
        let crate_item = LayoutItem::new(0, ItemKind::Crate, "my_crate");
        let module_item = LayoutItem::new(
            1,
            ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            "my_module",
        );
        assert_eq!(crate_item.label, "my_crate");
        assert_eq!(module_item.id, 1);
        match module_item.kind {
            ItemKind::Module { nesting, parent } => {
                assert_eq!(nesting, 1);
                assert_eq!(parent, 0);
            }
            _ => panic!("Expected Module"),
        }
    }

    #[test]
    fn test_layout_edge_kinds() {
        let normal = LayoutEdge::new(0, 1, EdgeContext::production());
        let direct =
            LayoutEdge::new(1, 0, EdgeContext::production()).with_cycle(CycleKind::Direct, vec![0]);
        let trans = LayoutEdge::new(2, 3, EdgeContext::production())
            .with_cycle(CycleKind::Transitive, vec![1]);

        assert_eq!(normal.from, 0);
        assert_eq!(direct.cycle, Some(CycleKind::Direct));
        assert_eq!(trans.cycle, Some(CycleKind::Transitive));
    }

    #[test]
    fn test_layout_ir_builder() {
        let mut ir = LayoutIR::new();

        let crate_id = ir.add_item(ItemKind::Crate, "my_crate".to_string());
        let mod_id = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: crate_id,
            },
            "my_module".to_string(),
        );
        ir.edges
            .push(LayoutEdge::new(crate_id, mod_id, EdgeContext::production()));

        assert_eq!(ir.items.len(), 2);
        assert_eq!(ir.edges.len(), 1);
        assert_eq!(ir.items[crate_id].label, "my_crate");
    }

    #[test]
    fn test_layout_item_defaults() {
        let mut ir = LayoutIR::new();
        let id = ir.add_item(ItemKind::Crate, "test".to_string());
        check!(ir.items[id].source_path.is_none());
        check!(ir.items[id].volatility.is_none());
    }

    #[test]
    fn test_nested_module_hierarchy_ordering() {
        // Setup: Crate mit nested Modulen
        // crate
        // ├── parent
        // │   ├── alpha_child
        // │   └── zebra_child
        // └── other_module (alphabetisch vor "parent", aber kein Kind)
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test_crate", &["other_module", "parent"])
            .nested_module("parent", "alpha_child")
            .nested_module("parent", "zebra_child");
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        let pos_parent = la.pos("parent");
        let pos_alpha = la.pos("alpha_child");
        let pos_zebra = la.pos("zebra_child");

        // Kinder MÜSSEN direkt nach Parent kommen
        assert!(
            pos_alpha > pos_parent && pos_alpha < pos_parent + 3,
            "alpha_child must directly follow parent. Labels: {:?}",
            la.labels()
        );
        assert!(
            pos_zebra > pos_parent && pos_zebra < pos_parent + 3,
            "zebra_child must directly follow parent. Labels: {:?}",
            la.labels()
        );

        // Alphabetisch innerhalb Geschwister
        la.assert_order("alpha_child", "zebra_child");
    }

    /// Module ordering: crate with N modules + deps → assert full sequence.
    ///
    /// - `topo_overrides_alphabetical`: zebra→beta→alpha reverses alphabetical (dependents first)
    /// - `topo_matches_alphabetical`: A→B→C — topo order matches alphabetical
    #[rstest]
    #[case::topo_overrides_alphabetical(
        &["alpha", "beta", "zebra"],
        &[("zebra", "beta"), ("beta", "alpha")],
        &["zebra", "beta", "alpha"]
    )]
    #[case::topo_matches_alphabetical(
        &["a", "b", "c"],
        &[("a", "b"), ("b", "c")],
        &["a", "b", "c"]
    )]
    fn test_module_ordering(
        #[case] modules: &[&str],
        #[case] deps: &[(&str, &str)],
        #[case] expected_order: &[&str],
    ) {
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test", modules);
        for &(from, to) in deps {
            b.prod_dep(from, to);
        }
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        la.assert_top_level_order(expected_order);
    }

    #[test]
    fn test_test_edges_do_not_affect_sibling_sort_order() {
        let mut b = TestGraphBuilder::new();
        // Production: alpha depends on beta → alpha should come first
        b.crate_with_modules("my_crate", &["alpha", "beta"])
            .prod_dep("alpha", "beta")
            // Test: beta depends on alpha (reverse direction) → must NOT affect order
            .test_dep("beta", "alpha", TestKind::Unit);
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        la.assert_order("alpha", "beta");
    }

    #[test]
    fn test_test_edges_do_not_affect_crate_sort_order() {
        let mut b = TestGraphBuilder::new();
        // Production: aaa depends on bbb → aaa first
        b.crate_with_modules("aaa", &["mod_a"])
            .crate_with_modules("bbb", &["mod_b"])
            .crate_dep("aaa", "bbb")
            // Test: bbb depends on aaa (reverse) → must NOT affect order
            .test_crate_dep("bbb", "aaa", TestKind::Unit);
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        la.assert_order("aaa", "bbb");
    }

    /// CrateDep edges are suppressed whenever a ModuleDep exists between the same crate pair,
    /// regardless of which node types (Crate or Module) the ModuleDep connects.
    ///
    /// - `module_to_entry_point`: mod_a (Module) → crate_b (Crate entry point)
    /// - `crate_to_crate`: crate_a (Crate root/lib.rs) → crate_b (Crate entry point)
    /// - `crate_to_module`: crate_a (Crate root/lib.rs) → mod_b (Module, not entry point)
    struct SuppressionCase {
        crate_a_modules: &'static [&'static str],
        crate_b_modules: &'static [&'static str],
        dep_from: &'static str,
        dep_to: &'static str,
    }

    impl SuppressionCase {
        fn build_layout(&self) -> LayoutIR {
            let mut b = TestGraphBuilder::new();
            b.crate_with_modules("crate_a", self.crate_a_modules)
                .crate_with_modules("crate_b", self.crate_b_modules)
                .crate_dep("crate_a", "crate_b")
                .prod_dep_located(self.dep_from, self.dep_to);
            let (graph, _) = b.build();
            super::build_layout(&graph, &[])
        }
    }

    #[rstest]
    #[case::module_to_entry_point(SuppressionCase {
        crate_a_modules: &["mod_a"], crate_b_modules: &[],
        dep_from: "mod_a", dep_to: "crate_b",
    })]
    #[case::crate_to_crate(SuppressionCase {
        crate_a_modules: &["dummy_a"], crate_b_modules: &["dummy_b"],
        dep_from: "crate_a", dep_to: "crate_b",
    })]
    #[case::crate_to_module(SuppressionCase {
        crate_a_modules: &["dummy_a"], crate_b_modules: &["mod_b"],
        dep_from: "crate_a", dep_to: "mod_b",
    })]
    fn test_crate_dep_suppressed(#[case] case: SuppressionCase) {
        let ir = case.build_layout();
        assert_eq!(ir.edges.len(), 1, "CrateDep should be suppressed");
        assert!(
            !ir.edges[0].source_locations.is_empty(),
            "Remaining edge should be ModuleDep"
        );
    }

    #[test]
    fn test_subtree_dependency_ordering() {
        // Setup: Two parent modules, each with a child.
        // parent_a::child_a depends on parent_b::child_b
        // Expected: parent_a before parent_b (subtree dependency aggregation)
        //
        // crate
        // ├── parent_a         <-- should come FIRST (its subtree depends on parent_b's subtree)
        // │   └── child_a      <-- depends on child_b
        // └── parent_b         <-- should come SECOND (dependency target)
        //     └── child_b      <-- used by child_a
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test_crate", &["parent_a", "parent_b"])
            .nested_module("parent_a", "child_a")
            .nested_module("parent_b", "child_b")
            .prod_dep("child_a", "child_b");
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        la.assert_order("parent_a", "parent_b");
    }

    #[test]
    fn test_crate_order_respects_inter_crate_module_deps() {
        // When modules in crate_a depend on modules in crate_b
        // but there is NO CrateDep edge, the crate ordering should still
        // place crate_a before crate_b (dependent crate first).
        //
        // crate_b added first (lower graph index) to expose index-order bugs.
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("crate_b", &["mod_b"])
            .crate_with_modules("crate_a", &["mod_a"])
            .prod_dep("mod_a", "mod_b"); // no CrateDep edge!
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        la.assert_order("crate_a", "crate_b");

        // The ModuleDep edge should be Downward (not Upward)
        let edge = la
            .ir
            .edges
            .iter()
            .find(|e| la.ir.items[e.from].label == "mod_a" && la.ir.items[e.to].label == "mod_b")
            .expect("Should have edge from mod_a to mod_b");
        assert_eq!(edge.direction, EdgeDirection::Downward);
        assert!(edge.cycle.is_none());
    }

    // === Cross-Subtree Cycle Tests (ca-0211) ===

    #[test]
    fn test_cross_subtree_cycles_weighted_asymmetric() {
        // Topology: 4 module groups under one crate
        //   A (A1, A2, A3)  — no cycle involvement
        //   B (B1, B2)      — B1 in both cycles
        //   C               — standalone, cycle with B1
        //   D (D1, D2, D3)  — D2 in cycle with B1
        //
        // Cycles: B1<->C, B1<->D2
        //
        // Additional asymmetric edges (non-cycle):
        //   D1 → B2   (D depends on B — extra weight D→B direction)
        //   D3 → C    (D depends on C — extra weight D→C direction)
        //
        // Weighted virtual edges at group level:
        //   w(D→B) = 2 (D2→B1 + D1→B2),  w(B→D) = 1 (B1→D2)
        //   w(D→C) = 1 (D3→C),            w(C→D) = 0
        //   w(B→C) = 1 (B1→C),            w(C→B) = 1 (C→B1)
        //
        // Upward edge counts per permutation:
        //   D,B,C → 2 upward (optimal)
        //   D,C,B → 2 upward (optimal, but lexicographically D,B,C wins)
        //   B,D,C → 3 upward
        //   C,D,B → 3 upward
        //   B,C,D → 4 upward (worst — current alphabetical behavior)
        //   C,B,D → 4 upward (worst)
        //
        // Expected SCC order: D, B, C (minimum upward, lexicographic tiebreak)
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test_crate", &["a", "b", "c", "d"])
            .nested_module("a", "a1")
            .nested_module("a", "a2")
            .nested_module("a", "a3")
            .nested_module("b", "b1")
            .nested_module("b", "b2")
            .nested_module("d", "d1")
            .nested_module("d", "d2")
            .nested_module("d", "d3")
            // Cycle 1: B1 <-> C
            .prod_dep("b1", "c")
            .prod_dep("c", "b1")
            // Cycle 2: B1 <-> D2
            .prod_dep("b1", "d2")
            .prod_dep("d2", "b1")
            // Asymmetric non-cycle edges: D's subtree uses B and C
            .prod_dep("d1", "b2")
            .prod_dep("d3", "c");
        let (graph, _) = b.build();
        let ir = build_layout(&graph, &[]);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();
        let top_level: Vec<&str> = labels
            .iter()
            .filter(|&&l| matches!(l, "a" | "b" | "c" | "d"))
            .copied()
            .collect();

        // With weighted virtual edges, D→B has weight 2 vs B→D weight 1,
        // and D→C has weight 1 vs C→D weight 0.
        // Optimal ordering within SCC: D, B, C (2 upward edges)
        // Alphabetical B, C, D would give 4 upward edges (worst case).
        assert_eq!(
            top_level,
            vec!["a", "d", "b", "c"],
            "Expected: A first (independent, Kahn's), then SCC ordered D,B,C \
             (minimum upward edges with weighted virtual edges). \
             w(D→B)=2 > w(B→D)=1 → D before B. \
             w(D→C)=1 > w(C→D)=0 → D before C. \
             w(B→C)=1 = w(C→B)=1 → alphabetical tiebreak B before C."
        );
    }

    // === Cycle-Breaking Tests (ca-0170) ===

    // === Barycenter Heuristic Tests (ca-0159) ===

    #[test]
    fn test_barycenter_reduces_crossings() {
        // Graph: A→D, B→C
        // Barycenter places each dep adjacent to its dependent.
        // d is interleaved before c because d's dependent (a) is already placed,
        // giving d a lower barycenter score (0.0) than c (whose dependent b has score 1.0).
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test", &["a", "b", "c", "d"])
            .prod_dep("a", "d")
            .prod_dep("b", "c");
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        la.assert_order("d", "c");
    }

    #[test]
    fn test_barycenter_symmetric_diamond() {
        // Symmetric diamond: A→C, A→D, B→C, B→D
        // Equal barycenter scores → alphabetical fallback
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("test", &["a", "b", "c", "d"])
            .prod_dep("a", "c")
            .prod_dep("a", "d")
            .prod_dep("b", "c")
            .prod_dep("b", "d");
        let (graph, _) = b.build();
        let la = LayoutAssert::new(build_layout(&graph, &[]));

        la.assert_order("a", "c");
        la.assert_order("b", "d");
        la.assert_order("a", "b");
    }
}
