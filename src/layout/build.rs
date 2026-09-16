//! Build layout IR from graph and cycle information.

use super::jump::{JumpTable, JumpTarget, LocatedDefinition, LocatedSource, TargetKind};
use super::toposort::stable_toposort;
use crate::diagnose::{
    Cluster, ConsumerLocality, Cycle, CycleAnalysis, CyclicEdge, order_cycle_blocks,
};
use crate::graph::{ArcGraph, EdgeWeight, Node, Reexports};
use crate::model::{EdgeContext, SourceLocation, TargetRoots};
use crate::volatility::Volatility;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Index into LayoutIR.items
pub type NodeId = usize;

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Crate,
    Module {
        nesting: u32,
        parent: NodeId,
    },
    ExternalSection,
    ExternalCrate {
        parent: NodeId,
        is_direct_dependency: bool,
    },
}

#[derive(Debug, Clone)]
pub struct LayoutItem {
    pub id: NodeId,
    pub kind: ItemKind,
    pub label: String,
    pub source_path: Option<String>,
    pub volatility: Option<(Volatility, usize)>,
    pub version: Option<String>,
    /// SCC id when this node lies in a dependency cycle, else `None`.
    pub scc_id: Option<usize>,
    /// This item's jump targets; empty for items with none (e.g. `ExternalSection`).
    pub(crate) targets: Vec<JumpTarget>,
}

impl LayoutItem {
    pub fn new(id: NodeId, kind: ItemKind, label: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            label: label.into(),
            source_path: None,
            volatility: None,
            version: None,
            scc_id: None,
            targets: Vec::new(),
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
    /// SCC id when this edge lies inside a cycle, else `None`.
    pub scc_id: Option<usize>,
    pub(crate) source_locations: Vec<LocatedSource>,
    pub context: EdgeContext,
    /// Edge republishes names via `pub use` only (no behavioral coupling).
    /// Rendered as a distinct teal, dashed, default-hidden arc.
    pub reexport: bool,
}

impl LayoutEdge {
    /// Create a new edge with auto-computed direction.
    /// `Downward` when `from < to`, `Upward` otherwise.
    #[must_use]
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
            scc_id: None,
            source_locations: vec![],
            context,
            reexport: false,
        }
    }

    #[must_use]
    pub fn with_cycle(mut self, kind: CycleKind, ids: Vec<usize>, scc_id: usize) -> Self {
        self.cycle = Some(kind);
        self.cycle_ids = ids;
        self.scc_id = Some(scc_id);
        self
    }

    /// Assign each location an id from `table` and attach them to the edge.
    /// Test-only: production edges get their located sources from
    /// `populate_edges`, which inserts into the table directly.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_source_locations(
        mut self,
        table: &mut JumpTable,
        locations: Vec<SourceLocation>,
    ) -> Self {
        self.source_locations = locate_sources(table, locations);
        self
    }
}

#[derive(Debug)]
pub struct CyclicEdgeInfo {
    pub from_id: NodeId,
    pub to_id: NodeId,
    pub symbols: usize,
}

#[derive(Debug)]
pub struct ClusterInfo {
    pub crate_name: String,
    pub module_count: usize,
    pub cycle_count: usize,
    /// Cycles as ordered edge blocks: `cycles[i]` holds one cycle's edges in
    /// node order, closing edge last. Blocks are sorted for
    /// top-to-bottom sidebar reading (start node's layout rank, then length,
    /// then rest-sequence rank).
    pub cycles: Vec<Vec<CyclicEdgeInfo>>,
}

/// One symbol of a provider: how close its consumers sit in the module tree,
/// plus the full consumer list, in layout `NodeId` space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLocality {
    pub locality: ConsumerLocality<NodeId>,
    pub consumers: Vec<NodeId>,
}

#[derive(Debug, Default)]
pub struct LayoutIR {
    pub items: Vec<LayoutItem>,
    pub edges: Vec<LayoutEdge>,
    pub clusters: BTreeMap<usize, ClusterInfo>,
    /// Provider `NodeId` → symbol → consumer locality. Providers with only
    /// re-export/test usage carry no entry.
    pub symbol_localities: BTreeMap<NodeId, BTreeMap<String, SymbolLocality>>,
    /// Provider `NodeId` → symbol → its definition site, for the symbols that
    /// cross an edge into the provider and that it defines.
    pub(crate) symbol_definitions: BTreeMap<NodeId, BTreeMap<String, LocatedDefinition>>,
}

impl LayoutIR {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_item(&mut self, kind: ItemKind, label: String) -> NodeId {
        let id = self.items.len();
        self.items.push(LayoutItem::new(id, kind, label));
        id
    }
}

/// Build `LayoutIR` from graph and cycle information, alongside the `JumpTable`
/// holding every item's jump targets.
/// Converts graph nodes to `LayoutItems` with proper nesting and edges with cycle markers,
/// and attaches each cyclic SCC's cluster (crate name, size, and cycles).
/// `CrateDep` edges are skipped when `ModuleDep` edges exist between the same crates.
/// `reexports` must match the subgraph `analysis` was computed from.
/// Target ids are assigned in item order, so runs on the same graph are deterministic.
/// Target names are the paths relative to `workspace_root`; without a root, or
/// for a path outside it, the absolute path.
#[must_use]
pub(crate) fn build_layout(
    graph: &ArcGraph,
    analysis: &CycleAnalysis,
    reexports: Reexports,
    workspace_root: Option<&Path>,
) -> (LayoutIR, JumpTable) {
    let mut ir = LayoutIR::new();
    let mut table = JumpTable::new();
    let edge_to_cycles = &analysis.edge_cycles;
    let parent_map = graph.parent_map();

    // 3-way partition: workspace crates, modules, external crates
    let mut crate_indices = Vec::new();
    let mut module_indices = Vec::new();
    let mut external_indices = Vec::new();
    for idx in graph.node_indices() {
        match &graph[idx] {
            Node::Crate { .. } => crate_indices.push(idx),
            Node::Module { .. } => module_indices.push(idx),
            Node::ExternalCrate { .. } => external_indices.push(idx),
        }
    }

    let crate_indices = graph.order_crates(&crate_indices);
    let reachable = graph.production_reachable();
    let ordered = graph.order_items(&crate_indices, &module_indices, &reachable);
    let mut node_map = populate_items(
        &mut ir,
        graph,
        &ordered,
        &parent_map,
        &analysis.node_scc,
        &mut table,
        workspace_root,
    );

    if !external_indices.is_empty() {
        populate_external_items(
            &mut ir,
            graph,
            &external_indices,
            &mut node_map,
            &mut table,
            workspace_root,
        );
    }

    let suppressed = graph.suppressed_crate_pairs();
    populate_edges(
        &mut ir,
        graph,
        &node_map,
        edge_to_cycles,
        &analysis.node_scc,
        &suppressed,
        &mut table,
    );

    attach_clusters(&mut ir, graph, analysis, &node_map, reexports);
    attach_symbol_localities(&mut ir, graph, &node_map);
    attach_symbol_definitions(&mut ir, graph, &node_map, &mut table);
    (ir, table)
}

/// Return the path shown in the UI for a jump target: relative to
/// `workspace_root` when it lies inside, otherwise as given.
fn target_name(path: &Path, workspace_root: Option<&Path>) -> String {
    workspace_root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Insert a crate's (or external crate's) jump targets: its lib root, then
/// each bin root, then its manifest, all at line 1.
fn crate_targets(
    table: &mut JumpTable,
    target_roots: &TargetRoots,
    manifest: &Path,
    workspace_root: Option<&Path>,
) -> Vec<JumpTarget> {
    let mut targets = Vec::new();
    if let Some(lib_root) = &target_roots.lib_root {
        targets.push(JumpTarget {
            kind: TargetKind::Lib,
            name: target_name(lib_root, workspace_root),
            id: table.insert(lib_root.clone(), 1),
        });
    }
    for bin_root in &target_roots.bin_roots {
        targets.push(JumpTarget {
            kind: TargetKind::Bin,
            name: target_name(bin_root, workspace_root),
            id: table.insert(bin_root.clone(), 1),
        });
    }
    targets.push(JumpTarget {
        kind: TargetKind::Manifest,
        name: target_name(manifest, workspace_root),
        id: table.insert(manifest.to_path_buf(), 1),
    });
    targets
}

/// Insert a module's declaring file as its jump target, when known.
fn module_targets(
    table: &mut JumpTable,
    file: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Vec<JumpTarget> {
    let Some(file) = file else {
        return Vec::new();
    };
    vec![JumpTarget {
        kind: TargetKind::Module,
        name: target_name(file, workspace_root),
        id: table.insert(file.to_path_buf(), 1),
    }]
}

/// Insert an edge's source locations and pair each with its assigned id, in
/// the order given.
fn locate_sources(table: &mut JumpTable, locations: Vec<SourceLocation>) -> Vec<LocatedSource> {
    locations
        .into_iter()
        .map(|location| {
            let id = table.insert(location.file.clone(), location.line);
            LocatedSource { location, id }
        })
        .collect()
}

/// Insert one jump target per `(provider, symbol)` for the symbols that cross
/// an edge into a module that defines them, in edge order. Ids are assigned
/// after the edges' own, so a run on the same graph stays deterministic.
fn attach_symbol_definitions(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    node_map: &HashMap<NodeIndex, NodeId>,
    table: &mut JumpTable,
) {
    let graph_index: HashMap<NodeId, NodeIndex> =
        node_map.iter().map(|(&idx, &id)| (id, idx)).collect();
    for edge in &ir.edges {
        let Some(Node::Module {
            file: Some(file),
            definitions,
            ..
        }) = graph_index.get(&edge.to).map(|&idx| &graph[idx])
        else {
            continue;
        };
        for symbol in edge
            .source_locations
            .iter()
            .flat_map(|source| &source.location.symbols)
        {
            let Some(definition) = definitions.get(symbol) else {
                continue;
            };
            ir.symbol_definitions
                .entry(edge.to)
                .or_default()
                .entry(symbol.clone())
                .or_insert_with(|| LocatedDefinition {
                    line: definition.line,
                    id: table.insert(file.clone(), definition.line),
                });
        }
    }
}

/// Translate `importer_partition` into layout `NodeId` space and attach it as a
/// per-symbol side table. Mirrors `attach_clusters`: provider/target/consumer
/// `NodeIndex` map through `node_map`; endpoints absent from the layout are
/// skipped. Each symbol of a consumer group gets the group's hint, so lookup is
/// by `(provider, symbol)`.
fn attach_symbol_localities(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    node_map: &HashMap<NodeIndex, NodeId>,
) {
    for provider in graph.importer_partition().providers {
        let Some(&provider_id) = node_map.get(&provider.module) else {
            continue;
        };
        let mut symbols: BTreeMap<String, SymbolLocality> = BTreeMap::new();
        for group in provider.consumer_groups {
            // Translate the common-home node into layout space; a node absent
            // from the layout degrades to `CrateWide` rather than dropping the
            // symbol.
            let locality = match group.locality {
                ConsumerLocality::SingleConsumer(n) => node_map
                    .get(&n)
                    .copied()
                    .map(ConsumerLocality::SingleConsumer),
                ConsumerLocality::CommonAncestor(m) => node_map
                    .get(&m)
                    .copied()
                    .map(ConsumerLocality::CommonAncestor),
                ConsumerLocality::CrateWide => Some(ConsumerLocality::CrateWide),
            }
            .unwrap_or(ConsumerLocality::CrateWide);
            let consumers: Vec<NodeId> = group
                .consumers
                .iter()
                .filter_map(|c| node_map.get(c).copied())
                .collect();
            for symbol in group.symbols {
                symbols.insert(
                    symbol,
                    SymbolLocality {
                        locality,
                        consumers: consumers.clone(),
                    },
                );
            }
        }
        ir.symbol_localities.insert(provider_id, symbols);
    }
}

/// Compute each cluster's cycle blocks and attach them keyed by the
/// analysis sccId (the same id carried by the layout items).
fn attach_clusters(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    analysis: &CycleAnalysis,
    node_map: &HashMap<NodeIndex, NodeId>,
    reexports: Reexports,
) {
    let sub = graph.production_subgraph(reexports);
    // The diagram shows the graph as it is; rule config plays no part in it.
    let report = graph.cluster_report(&sub, analysis, |_| false);
    for cluster in report.clusters {
        let Some(&scc_id) = cluster.nodes.first().and_then(|n| analysis.node_scc.get(n)) else {
            continue;
        };
        let cycles = cycle_blocks(&cluster, analysis, node_map);
        ir.clusters.insert(
            scc_id,
            ClusterInfo {
                crate_name: graph[cluster.crate_idx].name().to_string(),
                module_count: cluster.nodes.len(),
                cycle_count: cluster.cycles.len(),
                cycles,
            },
        );
    }
}

/// Translate a cluster's cycles into ordered edge blocks for the sidebar: each
/// block is one cycle's nodes, rotated to start at the node with the smallest
/// layout `NodeId` (the vertical order nodes are drawn in), closing edge last;
/// blocks are sorted the same way so the sidebar reads top to bottom with as
/// few jumps as possible. The per-edge symbol count is looked up from
/// `cluster.edges`, which already covers every SCC-internal edge, so a cycle
/// edge is always found there.
fn cycle_blocks(
    cluster: &Cluster,
    analysis: &CycleAnalysis,
    node_map: &HashMap<NodeIndex, NodeId>,
) -> Vec<Vec<CyclicEdgeInfo>> {
    let edge_info: HashMap<(NodeIndex, NodeIndex), &CyclicEdge> =
        cluster.edges.iter().map(|e| ((e.from, e.to), e)).collect();
    let raw_cycles: Vec<Vec<NodeIndex>> = cluster
        .cycles
        .iter()
        .filter_map(|&idx| analysis.cycles.get(idx))
        .map(|cycle| cycle.nodes.clone())
        .collect();
    let rank_of = |n: NodeIndex| node_map.get(&n).copied().unwrap_or(usize::MAX);

    order_cycle_blocks(&raw_cycles, rank_of)
        .into_iter()
        .map(|nodes| {
            Cycle { nodes }
                .edges()
                .filter_map(|(from, to)| {
                    let edge = edge_info.get(&(from, to))?;
                    Some(CyclicEdgeInfo {
                        from_id: *node_map.get(&from)?,
                        to_id: *node_map.get(&to)?,
                        symbols: edge.symbols,
                    })
                })
                .collect()
        })
        .collect()
}

/// Convert graph nodes to `LayoutItems`, returning the `NodeIndex` → `NodeId` map.
fn populate_items(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    ordered: &[NodeIndex],
    parent_map: &HashMap<NodeIndex, NodeIndex>,
    node_scc: &HashMap<NodeIndex, usize>,
    table: &mut JumpTable,
    workspace_root: Option<&Path>,
) -> HashMap<NodeIndex, NodeId> {
    let mut node_map = HashMap::new();
    for &idx in ordered {
        let (kind, label, source_path, targets) = match &graph[idx] {
            Node::Crate {
                name,
                target_roots,
                manifest,
                ..
            } => (
                ItemKind::Crate,
                name.clone(),
                None,
                crate_targets(table, target_roots, manifest, workspace_root),
            ),
            Node::Module { name, file, .. } => {
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
                    module_targets(table, file.as_deref(), workspace_root),
                )
            }
            Node::ExternalCrate { .. } => continue, // handled by populate_external_items
        };

        let layout_id = ir.add_item(kind, label);
        node_map.insert(idx, layout_id);
        ir.items[layout_id].source_path = source_path;
        ir.items[layout_id].scc_id = node_scc.get(&idx).copied();
        // Under the key `STATIC_DATA` writes for the node. Workspace nodes
        // only: an external crate's files lie outside the root the editor's
        // paths are resolved against.
        table.insert_node_files(
            &layout_id.to_string(),
            targets.iter().map(|target| target.id),
        );
        ir.items[layout_id].targets = targets;
    }
    node_map
}

/// Add external crate nodes to the layout IR, preceded by a section header.
/// Sorted topologically by inter-external `CrateDep` edges so that dependencies appear
/// below their dependents, eliminating upward edges.  Incoming edge count (descending)
/// serves as tiebreaker to preserve the "most-used first" aesthetic where topology
/// does not constrain ordering.
fn populate_external_items(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    external_indices: &[NodeIndex],
    node_map: &mut HashMap<NodeIndex, NodeId>,
    table: &mut JumpTable,
    workspace_root: Option<&Path>,
) {
    let section_id = ir.add_item(
        ItemKind::ExternalSection,
        "External Dependencies".to_string(),
    );

    // Build a subgraph of only external nodes + their CrateDep edges.
    let mut ext_graph: DiGraph<NodeIndex, usize> = DiGraph::new();
    let orig_to_local: HashMap<NodeIndex, NodeIndex> = external_indices
        .iter()
        .map(|&idx| (idx, ext_graph.add_node(idx)))
        .collect();

    for edge in graph.edge_references() {
        if edge.weight().is_production_crate_dep()
            && let (Some(&src), Some(&dst)) = (
                orig_to_local.get(&edge.source()),
                orig_to_local.get(&edge.target()),
            )
        {
            ext_graph.increment_edge(src, dst);
        }
    }

    // Incoming edge count per node (from full graph, not subgraph) for tiebreaking.
    let incoming_counts: HashMap<NodeIndex, usize> = external_indices
        .iter()
        .map(|&idx| {
            let count = graph
                .edges_directed(idx, petgraph::Direction::Incoming)
                .count();
            (idx, count)
        })
        .collect();

    // Topological sort with incoming-count-descending tiebreaker.
    // `stable_toposort` breaks ties by `get_name` (lexicographic ascending).
    // Encode inverted incoming count as zero-padded prefix so higher counts sort first.
    let sorted = stable_toposort(&ext_graph, |orig_idx| {
        let count = incoming_counts.get(&orig_idx).copied().unwrap_or(0);
        format!("{:010}_{}", usize::MAX - count, graph[orig_idx].name())
    });

    for &idx in &sorted {
        if let Node::ExternalCrate {
            name,
            version,
            is_direct_dependency,
            target_roots,
            manifest,
            ..
        } = &graph[idx]
        {
            let layout_id = ir.add_item(
                ItemKind::ExternalCrate {
                    parent: section_id,
                    is_direct_dependency: *is_direct_dependency,
                },
                name.clone(),
            );
            ir.items[layout_id].version = Some(version.clone());
            ir.items[layout_id].targets =
                crate_targets(table, target_roots, manifest, workspace_root);
            node_map.insert(idx, layout_id);
        }
    }
}

/// Add dependency edges (`CrateDep` and `ModuleDep`) to the layout IR.
/// `CrateDep` and `ModuleDep` arms are unified — they share direction + cycle computation
/// and only differ in suppression check and source locations.
fn populate_edges(
    ir: &mut LayoutIR,
    graph: &ArcGraph,
    node_map: &HashMap<NodeIndex, NodeId>,
    edge_to_cycles: &HashMap<(NodeIndex, NodeIndex), Vec<usize>>,
    node_scc: &HashMap<NodeIndex, usize>,
    suppressed: &HashSet<(NodeIndex, NodeIndex)>,
    table: &mut JumpTable,
) {
    for edge in graph.edge_references() {
        let (src, dst) = (edge.source(), edge.target());

        let (locations, context, is_module_dep) = match edge.weight() {
            EdgeWeight::CrateDep { context } => {
                if suppressed.contains(&(src, dst)) {
                    continue;
                }
                (vec![], context.clone(), false)
            }
            EdgeWeight::ModuleDep { locations, context } => {
                (locations.clone(), context.clone(), true)
            }
            EdgeWeight::Contains | EdgeWeight::DevDep => continue,
        };

        if let (Some(&from), Some(&to)) = (node_map.get(&src), node_map.get(&dst)) {
            let (cycle, cycle_ids, scc_id) =
                compute_cycle_info(src, dst, edge_to_cycles, node_scc, graph, is_module_dep);
            ir.edges.push(LayoutEdge {
                cycle,
                cycle_ids,
                scc_id,
                source_locations: locate_sources(table, locations),
                reexport: edge.weight().is_reexport_module_dep(),
                ..LayoutEdge::new(from, to, context)
            });
        }
    }
}

/// Find the source file path for a module by inspecting its outgoing `ModuleDep` edges.
fn module_source_path(graph: &ArcGraph, idx: NodeIndex) -> Option<String> {
    graph
        .edges_directed(idx, petgraph::Direction::Outgoing)
        .find_map(|edge| match edge.weight() {
            EdgeWeight::ModuleDep { locations, .. } => {
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

/// Determine cycle kind and cycle IDs for an edge.
/// `CrateDep` edges are always `Transitive` when in a cycle.
/// A `ModuleDep` edge sits on a `Direct` cycle when the two modules import each
/// other (`a <-> b`), and on a `Transitive` one when the cycle runs through
/// further modules (`a -> b -> c -> a`).
fn compute_cycle_info(
    src: NodeIndex,
    dst: NodeIndex,
    edge_to_cycles: &HashMap<(NodeIndex, NodeIndex), Vec<usize>>,
    node_scc: &HashMap<NodeIndex, usize>,
    graph: &ArcGraph,
    is_module_dep: bool,
) -> (Option<CycleKind>, Vec<usize>, Option<usize>) {
    let cycle_ids = edge_to_cycles.get(&(src, dst)).cloned().unwrap_or_default();
    if cycle_ids.is_empty() {
        return (None, cycle_ids, None);
    }

    let has_reverse_module_dep = is_module_dep
        && graph
            .find_edge(dst, src)
            .is_some_and(|ei| matches!(graph[ei], EdgeWeight::ModuleDep { .. }));

    let kind = if has_reverse_module_dep {
        CycleKind::Direct
    } else {
        CycleKind::Transitive
    };
    (Some(kind), cycle_ids, node_scc.get(&src).copied())
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
/// For each sibling, collects its full subtree and counts how many production `ModuleDep`
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
    /// Children are sorted topologically by `ModuleDep` edges, with alphabetical tie-breaker.
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

    /// Re-sort crates by aggregated inter-crate dependencies (`CrateDep` + `ModuleDep`).
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

    /// Find crate pairs where `ModuleDep` edges exist (so `CrateDep` can be suppressed).
    /// Entry-point imports create `ModuleDep` edges where one or both endpoints
    /// are `Node::Crate` (not just `Node::Module`), so we handle all combinations.
    fn suppressed_crate_pairs(&self) -> HashSet<(NodeIndex, NodeIndex)> {
        self.edge_references()
            .filter_map(|edge| match edge.weight() {
                EdgeWeight::ModuleDep { .. } => {
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
    use super::super::jump::{JumpTable, Location, LocationId};
    use super::*;
    use crate::diagnose::RepresentativeCycles;
    use crate::graph::{ArcGraph, EdgeWeight, Node};
    use crate::model::{
        DefKind, Definition, EdgeContext, SourceLocation, TargetRoots, TestKind, UsageKind,
    };
    use crate::test_support::{
        crate_node, crate_node_with_targets, module_node, module_node_with_file,
    };
    use assert2::check;
    use petgraph::graph::NodeIndex;
    use rstest::rstest;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    /// `CycleAnalysis` for graphs with no cycles.
    fn no_cycles() -> CycleAnalysis {
        CycleAnalysis {
            cycles: Vec::new(),
            edge_cycles: HashMap::new(),
            node_scc: HashMap::new(),
        }
    }

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
        /// Path is auto-generated as "/<`crate_name`>".
        fn crate_with_modules(&mut self, crate_name: &str, module_names: &[&str]) -> &mut Self {
            let crate_idx = self.graph.add_node(crate_node(crate_name));
            self.names.insert(crate_name.to_string(), crate_idx);
            for &mod_name in module_names {
                let mod_idx = self.graph.add_node(module_node(mod_name, crate_idx));
                self.names.insert(mod_name.to_string(), mod_idx);
                self.graph
                    .add_edge(crate_idx, mod_idx, EdgeWeight::Contains);
            }
            self
        }

        /// Add a module not attached to any crate (uses `NodeIndex::new(0)` as `crate_idx`).
        fn orphan_module(&mut self, name: &str) -> &mut Self {
            let idx = self.graph.add_node(module_node(name, NodeIndex::new(0)));
            self.names.insert(name.to_string(), idx);
            self
        }

        /// Add a nested module under an existing parent (module or crate), with Contains edge.
        fn nested_module(&mut self, parent: &str, child: &str) -> &mut Self {
            let parent_idx = self.names[parent];
            let crate_idx = match &self.graph[parent_idx] {
                Node::Module { crate_idx, .. } => *crate_idx,
                Node::Crate { .. } | Node::ExternalCrate { .. } => parent_idx,
            };
            let child_idx = self.graph.add_node(module_node(child, crate_idx));
            self.names.insert(child.to_string(), child_idx);
            self.graph
                .add_edge(parent_idx, child_idx, EdgeWeight::Contains);
            self
        }

        /// Add a crate with target roots and a manifest, no child modules.
        fn crate_with_targets(
            &mut self,
            name: &str,
            target_roots: TargetRoots,
            manifest: &str,
        ) -> &mut Self {
            let idx = self
                .graph
                .add_node(crate_node_with_targets(name, target_roots, manifest));
            self.names.insert(name.to_string(), idx);
            self
        }

        /// Add a module with a declaring file under an existing parent (module or crate).
        fn module_with_file(&mut self, parent: &str, name: &str, file: &str) -> &mut Self {
            let parent_idx = self.names[parent];
            let crate_idx = match &self.graph[parent_idx] {
                Node::Module { crate_idx, .. } => *crate_idx,
                Node::Crate { .. } | Node::ExternalCrate { .. } => parent_idx,
            };
            let idx = self
                .graph
                .add_node(module_node_with_file(name, crate_idx, file));
            self.names.insert(name.to_string(), idx);
            self.graph.add_edge(parent_idx, idx, EdgeWeight::Contains);
            self
        }

        /// Add a module node with a declaring file and public definitions.
        fn module_with_definitions(
            &mut self,
            parent: &str,
            name: &str,
            file: &str,
            definitions: &[(&str, usize)],
        ) -> &mut Self {
            let parent_idx = self.names[parent];
            let crate_idx = match &self.graph[parent_idx] {
                Node::Module { crate_idx, .. } => *crate_idx,
                Node::Crate { .. } | Node::ExternalCrate { .. } => parent_idx,
            };
            let definitions = definitions
                .iter()
                .map(|&(symbol, line)| {
                    (
                        symbol.to_string(),
                        Definition {
                            kind: DefKind::Struct,
                            line,
                        },
                    )
                })
                .collect();
            let idx = self.graph.add_node(Node::Module {
                name: name.to_string(),
                crate_idx,
                file: Some(PathBuf::from(file)),
                definitions,
            });
            self.names.insert(name.to_string(), idx);
            self.graph.add_edge(parent_idx, idx, EdgeWeight::Contains);
            self
        }

        /// Add a production `ModuleDep` edge (empty locations).
        fn prod_dep(&mut self, from: &str, to: &str) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                EdgeWeight::ModuleDep {
                    locations: vec![],
                    context: EdgeContext::production(),
                },
            );
            self
        }

        /// Add a test `ModuleDep` edge (empty locations).
        fn test_dep(&mut self, from: &str, to: &str, kind: TestKind) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                EdgeWeight::ModuleDep {
                    locations: vec![],
                    context: EdgeContext::test(kind),
                },
            );
            self
        }

        /// Add a production `CrateDep` edge.
        fn crate_dep(&mut self, from: &str, to: &str) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                EdgeWeight::CrateDep {
                    context: EdgeContext::production(),
                },
            );
            self
        }

        /// Add a test `CrateDep` edge.
        fn test_crate_dep(&mut self, from: &str, to: &str, kind: TestKind) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                EdgeWeight::CrateDep {
                    context: EdgeContext::test(kind),
                },
            );
            self
        }

        /// Add an external crate node.
        fn external_crate(&mut self, name: &str, version: &str) -> &mut Self {
            let idx = self.graph.add_node(Node::ExternalCrate {
                name: name.to_string(),
                version: version.to_string(),
                package_id: format!("{name}-pkg"),
                is_direct_dependency: true,
                target_roots: TargetRoots::default(),
                manifest: format!("/reg/{name}/Cargo.toml").into(),
            });
            self.names.insert(name.to_string(), idx);
            self
        }

        /// Add a `ModuleDep` with a source location.
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
                EdgeWeight::ModuleDep {
                    locations: vec![SourceLocation {
                        file: PathBuf::from(file),
                        line,
                        symbols: symbols
                            .iter()
                            .map(std::string::ToString::to_string)
                            .collect(),
                        module_path: module_path.to_string(),
                        via_reexport: false,
                    }],
                    context: EdgeContext::production(),
                },
            );
            self
        }

        /// Add a `ModuleDep` with a dummy source location (for suppression tests).
        fn prod_dep_located(&mut self, from: &str, to: &str) -> &mut Self {
            self.prod_dep_with_location(from, to, "src/dummy.rs", 1, &["Sym"], to)
        }

        /// Add a `ModuleDep` carrying the given source locations, unmodified.
        fn prod_dep_with_locations(
            &mut self,
            from: &str,
            to: &str,
            locations: Vec<SourceLocation>,
        ) -> &mut Self {
            let src = self.names[from];
            let dst = self.names[to];
            self.graph.add_edge(
                src,
                dst,
                EdgeWeight::ModuleDep {
                    locations,
                    context: EdgeContext::production(),
                },
            );
            self
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
        assert_eq!(prod_edge.context.kind, UsageKind::Production);

        let test_edge = LayoutEdge::new(0, 1, EdgeContext::test(TestKind::Unit));
        assert_eq!(test_edge.context.kind, UsageKind::Test(TestKind::Unit));
    }

    #[test]
    fn test_layout_edge_has_source_locations() {
        let mut table = JumpTable::new();
        let edge = LayoutEdge::new(0, 1, EdgeContext::production()).with_source_locations(
            &mut table,
            vec![SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 42,
                symbols: vec![],
                module_path: String::new(),
                via_reexport: false,
            }],
        );
        assert_eq!(edge.source_locations.len(), 1);
        assert_eq!(edge.source_locations[0].location.line, 42);
    }

    // === Build Layout Tests ===

    #[test]
    fn test_build_layout_single_crate() {
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("my_crate", &["mod_a", "mod_b"])
            .prod_dep("mod_a", "mod_b");
        let (graph, _) = b.build();
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);

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
        let analysis = graph
            .production_subgraph(Reexports::Included)
            .representative_cycles();
        let (ir, _) = build_layout(&graph, &analysis, Reexports::Excluded, None);

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
        let analysis = graph
            .production_subgraph(Reexports::Included)
            .representative_cycles();
        let (ir, _) = build_layout(&graph, &analysis, Reexports::Excluded, None);

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

        // Cycle edges carry an scc_id; the two independent cycles are two SCCs.
        for edge in &cycle_edges {
            check!(edge.scc_id.is_some());
        }
        let scc_ids: HashSet<usize> = cycle_edges.iter().filter_map(|e| e.scc_id).collect();
        check!(scc_ids.len() == 2);

        // Non-cycle edges have no scc_id.
        for edge in &non_cycle_edges {
            check!(edge.scc_id.is_none());
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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

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
        let direct = LayoutEdge::new(1, 0, EdgeContext::production()).with_cycle(
            CycleKind::Direct,
            vec![0],
            0,
        );
        let trans = LayoutEdge::new(2, 3, EdgeContext::production()).with_cycle(
            CycleKind::Transitive,
            vec![1],
            0,
        );

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
    fn build_layout_assigns_jump_targets_and_resolves_them() {
        let target_roots = TargetRoots {
            lib_root: Some(PathBuf::from("/ws/app/src/lib.rs")),
            bin_roots: vec![PathBuf::from("/ws/app/src/main.rs")],
        };
        let mut b = TestGraphBuilder::new();
        b.crate_with_targets("app", target_roots, "/ws/app/Cargo.toml")
            .module_with_file("app", "mod_a", "/ws/app/src/mod_a.rs");
        let (graph, _) = b.build();
        let (ir, table) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);

        let la = LayoutAssert::new(ir);
        let app_targets = &la.ir.items[la.pos("app")].targets;
        let mod_a_targets = &la.ir.items[la.pos("mod_a")].targets;

        let kinds: Vec<TargetKind> = app_targets.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![TargetKind::Lib, TargetKind::Bin, TargetKind::Manifest]
        );
        assert_eq!(
            mod_a_targets.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![TargetKind::Module]
        );

        assert_eq!(
            table.resolve(app_targets[0].id),
            Some(&Location {
                file: PathBuf::from("/ws/app/src/lib.rs"),
                line: 1
            })
        );
        assert_eq!(
            table.resolve(app_targets[1].id),
            Some(&Location {
                file: PathBuf::from("/ws/app/src/main.rs"),
                line: 1
            })
        );
        assert_eq!(
            table.resolve(app_targets[2].id),
            Some(&Location {
                file: PathBuf::from("/ws/app/Cargo.toml"),
                line: 1
            })
        );
        assert_eq!(
            table.resolve(mod_a_targets[0].id),
            Some(&Location {
                file: PathBuf::from("/ws/app/src/mod_a.rs"),
                line: 1
            })
        );

        let ids: HashSet<LocationId> = app_targets
            .iter()
            .chain(mod_a_targets.iter())
            .map(|t| t.id)
            .collect();
        assert_eq!(ids.len(), 4, "all four ids should be distinct");
    }

    /// Every file a node's jump targets point at maps back to that node,
    /// under the key `STATIC_DATA` uses for it.
    #[test]
    fn build_layout_maps_target_files_back_to_their_node() {
        let target_roots = TargetRoots {
            lib_root: Some(PathBuf::from("/ws/app/src/lib.rs")),
            bin_roots: vec![PathBuf::from("/ws/app/src/main.rs")],
        };
        let mut b = TestGraphBuilder::new();
        b.crate_with_targets("app", target_roots, "/ws/app/Cargo.toml")
            .module_with_file("app", "mod_a", "/ws/app/src/mod_a.rs");
        let (graph, _) = b.build();
        let (ir, table) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);

        let la = LayoutAssert::new(ir);
        let app_key = la.pos("app").to_string();
        let mod_a_key = la.pos("mod_a").to_string();

        assert_eq!(
            table.node_at(Path::new("/ws/app/src/lib.rs")),
            Some(app_key.as_str())
        );
        assert_eq!(
            table.node_at(Path::new("/ws/app/src/main.rs")),
            Some(app_key.as_str())
        );
        assert_eq!(
            table.node_at(Path::new("/ws/app/Cargo.toml")),
            Some(app_key.as_str())
        );
        assert_eq!(
            table.node_at(Path::new("/ws/app/src/mod_a.rs")),
            Some(mod_a_key.as_str())
        );
    }

    /// A symbol crossing an edge gets one jump id at the provider when the
    /// provider defines it; a symbol the provider does not define gets none.
    #[test]
    fn build_layout_assigns_jump_ids_to_symbol_definitions() {
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("app", &[])
            .module_with_definitions("app", "mod_a", "/ws/app/src/mod_a.rs", &[("Widget", 7)])
            .module_with_file("app", "mod_b", "/ws/app/src/mod_b.rs")
            .prod_dep_with_location(
                "mod_b",
                "mod_a",
                "src/mod_b.rs",
                3,
                &["Widget", "Other"],
                "mod_a",
            );
        let (graph, _) = b.build();
        let (ir, table) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

        let definitions = &la.ir.symbol_definitions[&la.pos("mod_a")];
        let widget = definitions.get("Widget").expect("Widget is defined");
        assert_eq!(widget.line, 7);
        assert_eq!(
            table.resolve(widget.id),
            Some(&Location {
                file: PathBuf::from("/ws/app/src/mod_a.rs"),
                line: 7
            })
        );
        assert!(!definitions.contains_key("Other"));
    }

    fn target_names(la: &LayoutAssert, label: &str) -> Vec<String> {
        la.ir.items[la.pos(label)]
            .targets
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    #[test]
    fn target_names_are_relative_to_the_workspace_root() {
        let target_roots = TargetRoots {
            lib_root: Some(PathBuf::from("/ws/app/src/lib.rs")),
            bin_roots: vec![PathBuf::from("/ws/app/src/bin/tool.rs")],
        };
        let mut b = TestGraphBuilder::new();
        b.crate_with_targets("app", target_roots, "/ws/app/Cargo.toml")
            .module_with_file("app", "mod_a", "/ws/app/src/foo/mod.rs");
        let (graph, _) = b.build();
        let (ir, _) = build_layout(
            &graph,
            &no_cycles(),
            Reexports::Excluded,
            Some(Path::new("/ws")),
        );
        let la = LayoutAssert::new(ir);

        assert_eq!(
            target_names(&la, "app"),
            vec!["app/src/lib.rs", "app/src/bin/tool.rs", "app/Cargo.toml"]
        );
        assert_eq!(target_names(&la, "mod_a"), vec!["app/src/foo/mod.rs"]);
    }

    #[test]
    fn target_names_stay_absolute_outside_the_workspace_or_without_a_root() {
        let target_roots = TargetRoots {
            lib_root: Some(PathBuf::from("/registry/dep-1.0/src/lib.rs")),
            bin_roots: vec![],
        };
        let mut b = TestGraphBuilder::new();
        b.crate_with_targets("dep", target_roots, "/registry/dep-1.0/Cargo.toml");
        let (graph, _) = b.build();

        let (ir, _) = build_layout(
            &graph,
            &no_cycles(),
            Reexports::Excluded,
            Some(Path::new("/ws")),
        );
        assert_eq!(
            target_names(&LayoutAssert::new(ir), "dep"),
            vec![
                "/registry/dep-1.0/src/lib.rs",
                "/registry/dep-1.0/Cargo.toml"
            ]
        );

        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        assert_eq!(
            target_names(&LayoutAssert::new(ir), "dep"),
            vec![
                "/registry/dep-1.0/src/lib.rs",
                "/registry/dep-1.0/Cargo.toml"
            ]
        );
    }

    #[test]
    fn build_layout_assigns_ids_to_edge_source_locations_without_orphans() {
        let target_roots = TargetRoots {
            lib_root: Some(PathBuf::from("/ws/app/src/lib.rs")),
            bin_roots: vec![],
        };
        let mut b = TestGraphBuilder::new();
        b.crate_with_targets("app", target_roots, "/ws/app/Cargo.toml")
            .module_with_file("app", "mod_a", "/ws/app/src/mod_a.rs")
            .module_with_file("app", "mod_b", "/ws/app/src/mod_b.rs")
            .prod_dep_with_locations(
                "mod_a",
                "mod_b",
                vec![
                    SourceLocation {
                        file: PathBuf::from("/ws/app/src/mod_a.rs"),
                        line: 10,
                        symbols: vec!["Foo".to_string()],
                        module_path: "mod_b".to_string(),
                        via_reexport: false,
                    },
                    SourceLocation {
                        file: PathBuf::from("/ws/app/src/mod_a.rs"),
                        line: 20,
                        symbols: vec!["Bar".to_string()],
                        module_path: "mod_b".to_string(),
                        via_reexport: false,
                    },
                ],
            );
        let (graph, _) = b.build();
        let (ir, table) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);

        let la = LayoutAssert::new(ir);
        let edge = la
            .ir
            .edges
            .iter()
            .find(|e| e.from == la.pos("mod_a") && e.to == la.pos("mod_b"))
            .expect("mod_a -> mod_b edge");
        assert_eq!(edge.source_locations.len(), 2);

        let location_ids: Vec<LocationId> =
            edge.source_locations.iter().map(|loc| loc.id).collect();
        assert_ne!(
            location_ids[0], location_ids[1],
            "the two locations get distinct ids"
        );
        assert_eq!(
            table.resolve(location_ids[0]),
            Some(&Location {
                file: PathBuf::from("/ws/app/src/mod_a.rs"),
                line: 10
            })
        );
        assert_eq!(
            table.resolve(location_ids[1]),
            Some(&Location {
                file: PathBuf::from("/ws/app/src/mod_a.rs"),
                line: 20
            })
        );

        let target_ids: HashSet<LocationId> = la
            .ir
            .items
            .iter()
            .flat_map(|item| item.targets.iter().map(|t| t.id))
            .collect();
        for id in &location_ids {
            assert!(
                !target_ids.contains(id),
                "edge location id should be distinct from item target ids"
            );
        }

        // No orphan entries: the table holds exactly the item targets plus
        // the located sources, nothing more.
        let total_targets: usize = la.ir.items.iter().map(|item| item.targets.len()).sum();
        let total_located: usize = la.ir.edges.iter().map(|e| e.source_locations.len()).sum();
        let expected_len = total_targets + total_located;
        for i in 0..expected_len {
            assert!(
                table.resolve(LocationId::from(i)).is_some(),
                "id {i} should resolve"
            );
        }
        assert!(
            table.resolve(LocationId::from(expected_len)).is_none(),
            "table should hold no entry beyond targets plus located sources"
        );
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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

        la.assert_order("aaa", "bbb");
    }

    /// `CrateDep` edges are suppressed whenever a `ModuleDep` exists between the same crate pair,
    /// regardless of which node types (Crate or Module) the `ModuleDep` connects.
    ///
    /// - `module_to_entry_point`: `mod_a` (Module) → `crate_b` (Crate entry point)
    /// - `crate_to_crate`: `crate_a` (Crate root/lib.rs) → `crate_b` (Crate entry point)
    /// - `crate_to_module`: `crate_a` (Crate root/lib.rs) → `mod_b` (Module, not entry point)
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
            super::build_layout(&graph, &no_cycles(), Reexports::Excluded, None).0
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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

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

    // === Cross-Subtree Cycle Tests ===

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);

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

    // === Barycenter Heuristic Tests ===

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

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
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

        la.assert_order("a", "c");
        la.assert_order("b", "d");
        la.assert_order("a", "b");
    }

    // === External Dependency Ordering Tests ===

    #[test]
    fn test_transitive_external_deps_no_upward_edges() {
        // Workspace crate depends on serde, serde depends on proc-macro2.
        // Old sort: proc-macro2 had more incoming edges → placed above serde → upward edge.
        // Fixed sort: topological order ensures serde (dependent) above proc-macro2 (leaf).
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("my_app", &[])
            .external_crate("serde", "1.0.0")
            .external_crate("proc-macro2", "1.0.0")
            // my_app depends on serde
            .crate_dep("my_app", "serde")
            // serde depends on proc-macro2 (transitive external dep)
            .crate_dep("serde", "proc-macro2");
        let (graph, _) = b.build();
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);

        // All CrateDep edges between external crates should be Downward
        for edge in &ir.edges {
            let from_item = &ir.items[edge.from];
            let to_item = &ir.items[edge.to];
            if matches!(from_item.kind, ItemKind::ExternalCrate { .. })
                && matches!(to_item.kind, ItemKind::ExternalCrate { .. })
            {
                check!(
                    edge.direction == EdgeDirection::Downward,
                    "Edge {} → {} should be Downward, was {:?}",
                    from_item.label,
                    to_item.label,
                    edge.direction,
                );
            }
        }

        // serde (dependent) must appear before proc-macro2 (leaf)
        let la = LayoutAssert::new(ir);
        la.assert_order("serde", "proc-macro2");
    }

    #[test]
    fn build_layout_orders_cycle_blocks_by_layout_rank() {
        // Two triangles sharing directed edge m0->m1: two 3-node cycles
        // through m0. The fully-cyclic sibling set's minimum-upward-edges
        // order places m1 first and m0 last (cutting only m0->m1 suffices
        // to make the group acyclic), so cycle blocks rotate to start at
        // m1, not m0.
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("app", &["m0", "m1", "m2", "m3"])
            .prod_dep("m0", "m1")
            .prod_dep("m1", "m2")
            .prod_dep("m2", "m0")
            .prod_dep("m1", "m3")
            .prod_dep("m3", "m0");
        let (graph, _) = b.build();
        let analysis = graph
            .production_subgraph(Reexports::Excluded)
            .representative_cycles();
        let (ir, _) = build_layout(&graph, &analysis, Reexports::Excluded, None);

        assert_eq!(ir.clusters.len(), 1, "one cyclic cluster expected");
        let cluster = ir.clusters.values().next().unwrap();
        assert_eq!(cluster.cycles.len(), 2, "two triangles through m0");

        let id_of = |name: &str| ir.items.iter().find(|i| i.label == name).unwrap().id;
        let (m0, m1, m2, m3) = (id_of("m0"), id_of("m1"), id_of("m2"), id_of("m3"));
        assert!(
            m1 < m2 && m2 < m3 && m3 < m0,
            "expected layout rank m1 < m2 < m3 < m0"
        );

        // Block 0: m1->m2->m0->m1, closing edge last. Rest-sequence rank
        // (m2 before m3) puts this block first.
        let block0 = &cluster.cycles[0];
        assert_eq!(block0.len(), 3);
        assert_eq!((block0[0].from_id, block0[0].to_id), (m1, m2));
        assert_eq!((block0[1].from_id, block0[1].to_id), (m2, m0));
        assert_eq!((block0[2].from_id, block0[2].to_id), (m0, m1));

        // Block 1: m1->m3->m0->m1, closing edge last.
        let block1 = &cluster.cycles[1];
        assert_eq!(block1.len(), 3);
        assert_eq!((block1[0].from_id, block1[0].to_id), (m1, m3));
        assert_eq!((block1[1].from_id, block1[1].to_id), (m3, m0));
        assert_eq!((block1[2].from_id, block1[2].to_id), (m0, m1));
    }

    #[test]
    fn test_external_deps_incoming_count_tiebreaker() {
        // Two external crates with no inter-external edges but different incoming counts.
        // Both are depended on by workspace crates, but tokio has more dependents.
        // Tiebreaker: higher incoming count → placed first.
        let mut b = TestGraphBuilder::new();
        b.crate_with_modules("app_a", &[])
            .crate_with_modules("app_b", &[])
            .external_crate("alpha_crate", "1.0.0")
            .external_crate("tokio", "1.0.0")
            // Both apps depend on tokio (2 incoming), only app_a depends on alpha_crate (1 incoming)
            .crate_dep("app_a", "tokio")
            .crate_dep("app_b", "tokio")
            .crate_dep("app_a", "alpha_crate");
        let (graph, _) = b.build();
        let (ir, _) = build_layout(&graph, &no_cycles(), Reexports::Excluded, None);
        let la = LayoutAssert::new(ir);

        // tokio (2 incoming) should appear before alpha_crate (1 incoming),
        // despite alpha_crate being alphabetically first.
        la.assert_order("tokio", "alpha_crate");
    }
}
