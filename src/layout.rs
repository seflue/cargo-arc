//! Layout IR & Algorithms

use crate::graph::{ArcGraph, Edge};
use crate::volatility::Volatility;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

/// Index into LayoutIR.items
pub type NodeId = usize;

/// A cycle in the module dependency graph (SCC with >1 node)
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    /// NodeIndices participating in this cycle
    pub nodes: Vec<NodeIndex>,
}

/// SCC condensation fallback for stable_toposort when cycles exist.
/// Applies ADR-018 pattern: condense → toposort → expand alphabetically.
fn scc_fallback_sort(
    graph: &DiGraph<NodeIndex, usize>,
    node_to_orig: &HashMap<petgraph::graph::NodeIndex, NodeIndex>,
    get_name: &dyn Fn(NodeIndex) -> String,
) -> Vec<NodeIndex> {
    use std::collections::BinaryHeap;

    let sccs = tarjan_scc(graph);

    // Build condensed DAG: each SCC becomes one node
    let mut condensed: DiGraph<Vec<petgraph::graph::NodeIndex>, ()> = DiGraph::new();
    let mut node_to_scc: HashMap<petgraph::graph::NodeIndex, petgraph::graph::NodeIndex> =
        HashMap::new();

    for scc in &sccs {
        let scc_idx = condensed.add_node(scc.clone());
        for &node in scc {
            node_to_scc.insert(node, scc_idx);
        }
    }

    // Transfer edges between SCCs (skip intra-SCC)
    for edge in graph.edge_references() {
        let src_scc = node_to_scc[&edge.source()];
        let dst_scc = node_to_scc[&edge.target()];
        if src_scc != dst_scc && !condensed.contains_edge(src_scc, dst_scc) {
            condensed.add_edge(src_scc, dst_scc, ());
        }
    }

    // Stable Kahn's toposort on condensed DAG with alphabetical tiebreaker
    let scc_order = {
        let mut in_deg: HashMap<petgraph::graph::NodeIndex, usize> = HashMap::new();
        for n in condensed.node_indices() {
            in_deg.insert(n, 0);
        }
        for e in condensed.edge_references() {
            *in_deg.get_mut(&e.target()).unwrap() += 1;
        }

        // Sort name = alphabetically first member
        let scc_sort_name = |scc_idx: petgraph::graph::NodeIndex| -> String {
            condensed[scc_idx]
                .iter()
                .map(|&n| get_name(node_to_orig[&n]))
                .min()
                .unwrap_or_default()
        };

        // Min-heap by name
        let mut heap: BinaryHeap<std::cmp::Reverse<(String, petgraph::graph::NodeIndex)>> =
            BinaryHeap::new();
        for (&n, &deg) in &in_deg {
            if deg == 0 {
                heap.push(std::cmp::Reverse((scc_sort_name(n), n)));
            }
        }

        let mut order = Vec::new();
        while let Some(std::cmp::Reverse((_, n))) = heap.pop() {
            order.push(n);
            for neighbor in condensed.neighbors(n) {
                let deg = in_deg.get_mut(&neighbor).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    heap.push(std::cmp::Reverse((scc_sort_name(neighbor), neighbor)));
                }
            }
        }
        order
    };

    // Log cycles (SCCs with >1 member)
    for &scc_idx in &scc_order {
        let members = &condensed[scc_idx];
        if members.len() > 1 {
            let names: Vec<String> = members
                .iter()
                .map(|&n| get_name(node_to_orig[&n]))
                .collect();
            tracing::debug!("SCC cycle ({} nodes): {}", members.len(), names.join(", "));
        }
    }

    // Expand: multi-member SCCs via optimal_scc_order, singletons directly
    scc_order
        .into_iter()
        .flat_map(|scc_idx| {
            let members = &condensed[scc_idx];
            if members.len() > 1 {
                optimal_scc_order(members, graph, node_to_orig, get_name)
            } else {
                vec![node_to_orig[&members[0]]]
            }
        })
        .collect()
}

/// Brute-force minimum-upward-permutation for SCC members.
/// Enumerates all permutations and picks the one with the lowest sum of
/// "upward" edge weights (edges from later → earlier in the permutation).
/// Falls back to alphabetical for n > 8 (40320 permutations limit).
fn optimal_scc_order(
    members: &[petgraph::graph::NodeIndex],
    graph: &DiGraph<NodeIndex, usize>,
    node_to_orig: &HashMap<petgraph::graph::NodeIndex, NodeIndex>,
    get_name: &dyn Fn(NodeIndex) -> String,
) -> Vec<NodeIndex> {
    use itertools::Itertools;

    let n = members.len();

    // Guard: too many permutations → fallback alphabetical
    if n > 8 {
        let mut result: Vec<NodeIndex> = members.iter().map(|&m| node_to_orig[&m]).collect();
        result.sort_by_key(|&idx| get_name(idx));
        return result;
    }

    // Extract pairwise weights: weights[i][j] = edge weight from members[i] to members[j]
    let mut weights = vec![vec![0usize; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i != j
                && let Some(edge_idx) = graph.find_edge(members[i], members[j])
            {
                weights[i][j] = graph[edge_idx];
            }
        }
    }

    // Enumerate all permutations, score by upward weight sum
    let mut best_perm: Vec<usize> = (0..n).collect();
    let mut best_score = usize::MAX;
    let mut best_names: Vec<String> = Vec::new();

    for perm in (0..n).permutations(n) {
        // Score: sum of weights[perm[later]][perm[earlier]] for all later > earlier
        let mut score = 0usize;
        for later in 0..n {
            for earlier in 0..later {
                score += weights[perm[later]][perm[earlier]];
            }
        }

        if score < best_score
            || (score == best_score && {
                let names: Vec<String> = perm
                    .iter()
                    .map(|&i| get_name(node_to_orig[&members[i]]))
                    .collect();
                names < best_names
            })
        {
            best_score = score;
            best_names = perm
                .iter()
                .map(|&i| get_name(node_to_orig[&members[i]]))
                .collect();
            best_perm = perm;
        }
    }

    best_perm
        .iter()
        .map(|&i| node_to_orig[&members[i]])
        .collect()
}

/// Stable topological sort using Kahn's algorithm.
/// Preserves alphabetical order for nodes without dependency relationships.
fn stable_toposort(
    sibling_deps: &DiGraph<NodeIndex, usize>,
    children: &[NodeIndex],
    get_name: impl Fn(NodeIndex) -> String,
) -> Vec<NodeIndex> {
    use std::collections::BinaryHeap;

    if children.is_empty() {
        return vec![];
    }

    // Map sibling_deps node indices to original NodeIndex
    let node_to_orig: HashMap<petgraph::graph::NodeIndex, NodeIndex> = sibling_deps
        .node_indices()
        .map(|n| (n, sibling_deps[n]))
        .collect();

    // Compute in-degrees
    let mut in_degree: HashMap<petgraph::graph::NodeIndex, usize> = HashMap::new();
    for n in sibling_deps.node_indices() {
        in_degree.insert(n, 0);
    }
    for edge in sibling_deps.edge_references() {
        *in_degree.get_mut(&edge.target()).unwrap() += 1;
    }

    // Barycenter score: average position of already-placed dependents.
    // Nodes closer to their dependents get lower scores → fewer arc crossings.
    let barycenter = |n: petgraph::graph::NodeIndex,
                      positions: &HashMap<petgraph::graph::NodeIndex, usize>|
     -> f64 {
        let placed: Vec<usize> = sibling_deps
            .neighbors_directed(n, petgraph::Direction::Incoming)
            .filter_map(|pred| positions.get(&pred).copied())
            .collect();
        if placed.is_empty() {
            f64::INFINITY
        } else {
            placed.iter().sum::<usize>() as f64 / placed.len() as f64
        }
    };

    // Use BinaryHeap with barycenter score as primary key, name as tiebreaker
    struct Item(f64, String, petgraph::graph::NodeIndex);
    impl PartialEq for Item {
        fn eq(&self, other: &Self) -> bool {
            self.0.to_bits() == other.0.to_bits() && self.1 == other.1
        }
    }
    impl Eq for Item {}
    impl Ord for Item {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            // Reversed for min-heap: lower score = higher priority
            other
                .0
                .partial_cmp(&self.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| other.1.cmp(&self.1))
        }
    }
    impl PartialOrd for Item {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    // Position tracking: maps sibling_deps node index → placement order in result
    let mut positions: HashMap<petgraph::graph::NodeIndex, usize> = HashMap::new();

    // Initialize with nodes having in-degree 0 (all INFINITY — nothing placed yet)
    let mut heap: BinaryHeap<Item> = BinaryHeap::new();
    for (&n, &deg) in &in_degree {
        if deg == 0 {
            let orig = node_to_orig[&n];
            heap.push(Item(f64::INFINITY, get_name(orig), n));
        }
    }

    let mut result = Vec::new();
    while let Some(Item(_, _, n)) = heap.pop() {
        positions.insert(n, result.len());
        result.push(node_to_orig[&n]);

        // Decrease in-degree of neighbors
        for neighbor in sibling_deps.neighbors(n) {
            let deg = in_degree.get_mut(&neighbor).unwrap();
            *deg -= 1;
            if *deg == 0 {
                let orig = node_to_orig[&neighbor];
                let score = barycenter(neighbor, &positions);
                heap.push(Item(score, get_name(orig), neighbor));
            }
        }
    }

    // If not all nodes processed, there's a cycle — use SCC fallback
    if result.len() != children.len() {
        return scc_fallback_sort(sibling_deps, &node_to_orig, &get_name);
    }

    result
}

/// Collect all descendants of a node (including itself) via Contains edges.
fn collect_subtree(node: NodeIndex, graph: &ArcGraph) -> HashSet<NodeIndex> {
    let mut subtree = HashSet::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if subtree.insert(n) {
            for edge in graph.edges(n) {
                if matches!(edge.weight(), Edge::Contains) {
                    stack.push(edge.target());
                }
            }
        }
    }
    subtree
}

/// Hierarchically sorted modules for a parent, collecting children recursively.
/// Children are sorted topologically by ModuleDep edges, with alphabetical tie-breaker.
/// Also considers cross-subtree dependencies: if any node in subtree(A) depends on
/// any node in subtree(B), then A should appear before B.
fn collect_children_recursive(
    parent_idx: NodeIndex,
    graph: &ArcGraph,
    module_indices: &[NodeIndex],
    added: &mut HashSet<NodeIndex>,
) -> Vec<NodeIndex> {
    use crate::graph::Node;

    let mut result = Vec::new();

    // Find direct children of this parent (via Contains edge)
    let mut children: Vec<NodeIndex> = module_indices
        .iter()
        .filter(|&&m| {
            !added.contains(&m)
                && graph
                    .edges(parent_idx)
                    .any(|e| e.target() == m && matches!(e.weight(), Edge::Contains))
        })
        .copied()
        .collect();

    // FIRST: Sort alphabetically (provides stable base order for toposort)
    children.sort_by_key(|&idx| {
        if let Node::Module { name, .. } = &graph[idx] {
            name.clone()
        } else {
            String::new()
        }
    });

    // Collect subtrees for each sibling (child + all descendants)
    let mut subtrees: HashMap<NodeIndex, HashSet<NodeIndex>> = HashMap::new();
    for &child in &children {
        subtrees.insert(child, collect_subtree(child, graph));
    }

    // Build mini dependency graph for siblings using subtree-aggregated dependencies
    let mut sibling_deps: DiGraph<NodeIndex, usize> = DiGraph::new();
    let mut idx_to_node: HashMap<NodeIndex, petgraph::graph::NodeIndex> = HashMap::new();

    for &child in &children {
        idx_to_node.insert(child, sibling_deps.add_node(child));
    }

    // Find cross-subtree dependencies: if any node in subtree(child) depends on
    // any node in subtree(sibling), add edge child -> sibling
    for &child in &children {
        let child_subtree = &subtrees[&child];
        for &node in child_subtree {
            for edge in graph.edges(node) {
                if matches!(
                    edge.weight(),
                    Edge::ModuleDep {
                        context: crate::model::EdgeContext::Production,
                        ..
                    }
                ) {
                    let target = edge.target();
                    // Find which sibling's subtree contains the target
                    for (&sibling, sibling_subtree) in &subtrees {
                        if sibling != child && sibling_subtree.contains(&target) {
                            // child's subtree depends on sibling's subtree
                            let src = idx_to_node[&child];
                            let dst = idx_to_node[&sibling];
                            if let Some(edge_idx) = sibling_deps.find_edge(src, dst) {
                                sibling_deps[edge_idx] += 1;
                            } else {
                                sibling_deps.add_edge(src, dst, 1);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    // THEN: Stable topological sort using Kahn's algorithm
    // This preserves alphabetical order for independent nodes (tie-breaker)
    let sorted = stable_toposort(&sibling_deps, &children, |idx| match &graph[idx] {
        Node::Module { name, .. } => name.clone(),
        _ => String::new(),
    });
    if !sorted.is_empty() {
        children = sorted;
    }
    // On cycles (empty result): keep alphabetical order

    // Add each child + its descendants recursively
    for child in children {
        added.insert(child);
        result.push(child);
        result.extend(collect_children_recursive(
            child,
            graph,
            module_indices,
            added,
        ));
    }

    result
}

/// Build LayoutIR from graph and cycle information.
/// Converts graph nodes to LayoutItems with proper nesting and edges with cycle markers.
/// CrateDep edges are skipped when ModuleDep edges exist between the same crates.
pub fn build_layout(graph: &ArcGraph, cycles: &[Cycle]) -> LayoutIR {
    use crate::graph::Node;
    use std::collections::{HashMap, HashSet};

    let mut ir = LayoutIR::new();

    // Build set of cycle member pairs for quick lookup
    let cycle_pairs: HashSet<(NodeIndex, NodeIndex)> = cycles
        .iter()
        .flat_map(|c| {
            c.nodes
                .iter()
                .flat_map(|&a| c.nodes.iter().map(move |&b| (a, b)))
        })
        .collect();

    // Map graph NodeIndex to LayoutIR NodeId
    let mut node_map: HashMap<NodeIndex, NodeId> = HashMap::new();

    // Build parent map from Contains edges
    let mut parent_map: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    for edge_idx in graph.edge_indices() {
        if matches!(graph[edge_idx], Edge::Contains) {
            let (parent, child) = graph.edge_endpoints(edge_idx).unwrap();
            parent_map.insert(child, parent);
        }
    }

    // Calculate nesting depth for a node
    let calc_nesting = |idx: NodeIndex, parent_map: &HashMap<NodeIndex, NodeIndex>| -> u32 {
        let mut depth = 0u32;
        let mut current = idx;
        while let Some(&parent) = parent_map.get(&current) {
            depth += 1;
            current = parent;
        }
        depth
    };

    // Separate crates from modules
    let (crate_indices, module_indices): (Vec<NodeIndex>, Vec<NodeIndex>) = graph
        .node_indices()
        .partition(|&idx| matches!(graph[idx], Node::Crate { .. }));

    // Re-sort crates by aggregated inter-crate dependencies (CrateDep + ModuleDep).
    // topo_sort only constrains module ordering; crate nodes may float freely.
    let crate_indices = {
        let crate_of = |idx: NodeIndex| -> NodeIndex {
            match &graph[idx] {
                Node::Module { crate_idx, .. } => *crate_idx,
                Node::Crate { .. } => idx,
            }
        };

        // Build crate-level dependency graph
        let mut crate_graph: DiGraph<NodeIndex, usize> = DiGraph::new();
        let mut crate_to_node: HashMap<NodeIndex, petgraph::graph::NodeIndex> = HashMap::new();
        // Add crate nodes in deterministic order (by graph index)
        let mut sorted_crates = crate_indices.clone();
        sorted_crates.sort_by_key(|n| n.index());
        for &ci in &sorted_crates {
            crate_to_node.insert(ci, crate_graph.add_node(ci));
        }

        // Add edges from both CrateDep and ModuleDep (aggregated to crate level)
        for edge_idx in graph.edge_indices() {
            match &graph[edge_idx] {
                Edge::CrateDep {
                    context: crate::model::EdgeContext::Production,
                }
                | Edge::ModuleDep {
                    context: crate::model::EdgeContext::Production,
                    ..
                } => {
                    let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
                    let src_crate = crate_of(src);
                    let dst_crate = crate_of(dst);
                    if src_crate != dst_crate
                        && let (Some(&sc), Some(&dc)) =
                            (crate_to_node.get(&src_crate), crate_to_node.get(&dst_crate))
                    {
                        if let Some(edge_idx) = crate_graph.find_edge(sc, dc) {
                            crate_graph[edge_idx] += 1;
                        } else {
                            crate_graph.add_edge(sc, dc, 1);
                        }
                    }
                }
                _ => {}
            }
        }

        // Stable toposort with alphabetical tie-breaking
        stable_toposort(&crate_graph, &sorted_crates, |idx| match &graph[idx] {
            Node::Crate { name, .. } | Node::Module { name, .. } => name.clone(),
        })
    };

    // Group modules by their parent crate for proper visual grouping
    // Each crate is followed by its modules before the next crate
    let mut ordered_items: Vec<NodeIndex> = Vec::new();
    let mut added_modules: HashSet<NodeIndex> = HashSet::new();

    for crate_idx in &crate_indices {
        ordered_items.push(*crate_idx);
        // Hierarchically sorted modules for this crate
        let sorted_modules =
            collect_children_recursive(*crate_idx, graph, &module_indices, &mut added_modules);
        ordered_items.extend(sorted_modules);
    }

    // Add any remaining modules (orphans or modules with crate not in order list)
    for module_idx in &module_indices {
        if !added_modules.contains(module_idx) {
            ordered_items.push(*module_idx);
        }
    }

    // Add items in grouped order
    for &idx in &ordered_items {
        let (kind, label) = match &graph[idx] {
            Node::Crate { name, .. } => (ItemKind::Crate, name.clone()),
            Node::Module { name, .. } => {
                let nesting = calc_nesting(idx, &parent_map);
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
                )
            }
        };

        let layout_id = ir.add_item(kind, label);
        node_map.insert(idx, layout_id);

        // Extract source file path for modules from outgoing ModuleDep edges
        if matches!(&graph[idx], Node::Module { .. }) {
            let source_path = graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
                .filter_map(|e| match e.weight() {
                    Edge::ModuleDep {
                        locations: locs, ..
                    } => locs.first().map(|l| l.file.display().to_string()),
                    _ => None,
                })
                .next();
            ir.items[layout_id].source_path = source_path;
        }
    }

    // Build set of crate pairs that have ModuleDep edges between them.
    // Entry-point imports create ModuleDep edges where one or both endpoints
    // are Node::Crate (not just Node::Module), so we handle all combinations.
    let crate_of = |node_idx: NodeIndex| -> NodeIndex {
        match &graph[node_idx] {
            Node::Module { crate_idx, .. } => *crate_idx,
            Node::Crate { .. } => node_idx,
        }
    };
    let mut crates_with_module_deps: HashSet<(NodeIndex, NodeIndex)> = HashSet::new();
    for edge_idx in graph.edge_indices() {
        if let Edge::ModuleDep { .. } = &graph[edge_idx] {
            let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
            let src_crate = crate_of(src);
            let dst_crate = crate_of(dst);
            if src_crate != dst_crate {
                crates_with_module_deps.insert((src_crate, dst_crate));
            }
        }
    }

    // Add dependency edges (CrateDep and ModuleDep only)
    for edge_idx in graph.edge_indices() {
        match &graph[edge_idx] {
            Edge::CrateDep { context } => {
                let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
                // Skip CrateDep if ModuleDeps already show this relationship
                if crates_with_module_deps.contains(&(src, dst)) {
                    continue;
                }
                if let (Some(&from), Some(&to)) = (node_map.get(&src), node_map.get(&dst)) {
                    let kind = if cycle_pairs.contains(&(src, dst)) {
                        EdgeKind::TransitiveCycle
                    } else if from < to {
                        // from appears before to in topo order → normal downward flow
                        EdgeKind::Downward
                    } else {
                        // from appears after to → upward reference (child→parent)
                        EdgeKind::Upward
                    };
                    let is_test = !matches!(context, crate::model::EdgeContext::Production);
                    ir.add_edge(from, to, kind, vec![], is_test);
                }
            }
            Edge::ModuleDep { locations, context } => {
                let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
                if let (Some(&from), Some(&to)) = (node_map.get(&src), node_map.get(&dst)) {
                    let kind = if cycle_pairs.contains(&(src, dst)) {
                        // Check if it's a direct cycle (A->B and B->A both exist)
                        if cycle_pairs.contains(&(dst, src))
                            && graph.contains_edge(dst, src)
                            && matches!(
                                graph[graph.find_edge(dst, src).unwrap()],
                                Edge::ModuleDep { .. }
                            )
                        {
                            EdgeKind::DirectCycle
                        } else {
                            EdgeKind::TransitiveCycle
                        }
                    } else if from < to {
                        // from appears before to in topo order → normal downward flow
                        EdgeKind::Downward
                    } else {
                        // from appears after to → upward reference (child→parent)
                        EdgeKind::Upward
                    };
                    let is_test = !matches!(context, crate::model::EdgeContext::Production);
                    ir.add_edge(from, to, kind, locations.clone(), is_test);
                }
            }
            Edge::Contains => {} // Skip hierarchy edges
        }
    }

    ir
}

/// Detect cycles in module dependencies using Tarjan's SCC algorithm.
/// Only considers ModuleDep edges (Rust prevents crate-level cycles).
pub fn detect_cycles(graph: &ArcGraph) -> Vec<Cycle> {
    // Build filtered graph with only ModuleDep edges
    let mut filtered = ArcGraph::new();
    let mut node_map = std::collections::HashMap::new();

    // Copy all nodes
    for idx in graph.node_indices() {
        let new_idx = filtered.add_node(graph[idx].clone());
        node_map.insert(idx, new_idx);
    }

    // Copy only Production ModuleDep edges
    for edge_idx in graph.edge_indices() {
        if matches!(
            &graph[edge_idx],
            Edge::ModuleDep {
                context: crate::model::EdgeContext::Production,
                ..
            }
        ) {
            let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
            filtered.add_edge(
                node_map[&src],
                node_map[&dst],
                Edge::ModuleDep {
                    locations: vec![],
                    context: crate::model::EdgeContext::Production,
                },
            );
        }
    }

    // Run Tarjan's SCC algorithm
    let sccs = tarjan_scc(&filtered);

    // Filter to SCCs with >1 node (actual cycles)
    // Map back to original graph indices
    let reverse_map: std::collections::HashMap<_, _> =
        node_map.iter().map(|(k, v)| (*v, *k)).collect();

    sccs.into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| Cycle {
            nodes: scc.into_iter().map(|idx| reverse_map[&idx]).collect(),
        })
        .collect()
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeKind {
    Downward,        // Normal flow: Parent→Child (from_idx < to_idx in topo order)
    Upward,          // Tighter coupling: Child→Parent (from_idx > to_idx in topo order)
    DirectCycle,     // A⇄B bidirectional
    TransitiveCycle, // Part of larger cycle
}

use crate::model::SourceLocation;

#[derive(Debug, Clone)]
pub struct LayoutEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub source_locations: Vec<SourceLocation>,
    pub is_test: bool,
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
        self.items.push(LayoutItem {
            id,
            kind,
            label,
            source_path: None,
            volatility: None,
        });
        id
    }

    pub fn add_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
        source_locations: Vec<SourceLocation>,
        is_test: bool,
    ) {
        self.edges.push(LayoutEdge {
            from,
            to,
            kind,
            source_locations,
            is_test,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ArcGraph, Edge, Node};
    use crate::model::SourceLocation;
    use petgraph::graph::NodeIndex;
    use std::path::PathBuf;

    #[test]
    fn test_layout_edge_has_source_locations() {
        let edge = LayoutEdge {
            from: 0,
            to: 1,
            kind: EdgeKind::Downward,
            source_locations: vec![SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 42,
                symbols: vec![],
                module_path: String::new(),
            }],
            is_test: false,
        };
        assert_eq!(edge.source_locations.len(), 1);
        assert_eq!(edge.source_locations[0].line, 42);
    }

    // === Cycle Detection Tests ===

    #[test]
    fn test_no_cycles() {
        // A -> B -> C (linear, no cycle)
        let mut graph = ArcGraph::new();
        let a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let c = graph.add_node(Node::Module {
            name: "c".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        graph.add_edge(
            a,
            b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles = detect_cycles(&graph);
        assert!(cycles.is_empty(), "Linear graph should have no cycles");
    }

    #[test]
    fn test_direct_cycle() {
        // A <-> B (direct cycle between two modules)
        let mut graph = ArcGraph::new();
        let a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        graph.add_edge(
            a,
            b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            a,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles = detect_cycles(&graph);
        assert_eq!(cycles.len(), 1, "Should detect one cycle");
        assert_eq!(cycles[0].nodes.len(), 2, "Cycle should contain 2 nodes");
        assert!(cycles[0].nodes.contains(&a));
        assert!(cycles[0].nodes.contains(&b));
    }

    #[test]
    fn test_transitive_cycle() {
        // A -> B -> C -> A (transitive cycle)
        let mut graph = ArcGraph::new();
        let a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let c = graph.add_node(Node::Module {
            name: "c".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        graph.add_edge(
            a,
            b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            c,
            a,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles = detect_cycles(&graph);
        assert_eq!(cycles.len(), 1, "Should detect one cycle");
        assert_eq!(cycles[0].nodes.len(), 3, "Cycle should contain 3 nodes");
    }

    #[test]
    fn test_multiple_independent_cycles() {
        // A <-> B (cycle 1), C <-> D (cycle 2)
        // Two independent cycles that should both be detected
        let mut graph = ArcGraph::new();
        let a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let c = graph.add_node(Node::Module {
            name: "c".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let d = graph.add_node(Node::Module {
            name: "d".to_string(),
            crate_idx: NodeIndex::new(0),
        });

        // Cycle 1: A <-> B
        graph.add_edge(
            a,
            b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            a,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        // Cycle 2: C <-> D
        graph.add_edge(
            c,
            d,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            d,
            c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles = detect_cycles(&graph);
        assert_eq!(cycles.len(), 2, "Should detect two independent cycles");

        // Each cycle should have 2 nodes
        for cycle in &cycles {
            assert_eq!(cycle.nodes.len(), 2, "Each cycle should contain 2 nodes");
        }

        // Verify both cycles are detected (order may vary)
        let all_nodes: Vec<_> = cycles.iter().flat_map(|c| c.nodes.iter()).collect();
        assert!(all_nodes.contains(&&a), "Cycle 1 should contain A");
        assert!(all_nodes.contains(&&b), "Cycle 1 should contain B");
        assert!(all_nodes.contains(&&c), "Cycle 2 should contain C");
        assert!(all_nodes.contains(&&d), "Cycle 2 should contain D");
    }

    // === Build Layout Tests ===

    #[test]
    fn test_build_layout_single_crate() {
        use std::path::PathBuf;

        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path"),
        });
        let mod_a = graph.add_node(Node::Module {
            name: "mod_a".to_string(),
            crate_idx,
        });
        let mod_b = graph.add_node(Node::Module {
            name: "mod_b".to_string(),
            crate_idx,
        });
        graph.add_edge(crate_idx, mod_a, Edge::Contains);
        graph.add_edge(crate_idx, mod_b, Edge::Contains);
        graph.add_edge(
            mod_a,
            mod_b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        // Should have 3 items (1 crate + 2 modules)
        assert_eq!(ir.items.len(), 3);

        // Should have 1 dependency edge (mod_a -> mod_b)
        assert_eq!(ir.edges.len(), 1);
        assert!(matches!(ir.edges[0].kind, EdgeKind::Downward));
    }

    #[test]
    fn test_build_layout_with_cycles() {
        let mut graph = ArcGraph::new();
        let a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        let b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx: NodeIndex::new(0),
        });
        graph.add_edge(
            a,
            b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            a,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        ); // cycle

        let cycles = detect_cycles(&graph);
        let ir = build_layout(&graph, &cycles);

        // Should have 2 items
        assert_eq!(ir.items.len(), 2);

        // Should have 2 edges, both marked as cycle edges
        assert_eq!(ir.edges.len(), 2);
        for edge in &ir.edges {
            assert!(
                matches!(edge.kind, EdgeKind::DirectCycle | EdgeKind::TransitiveCycle),
                "Cycle edges should be marked"
            );
        }
    }

    #[test]
    fn test_upward_edge_direction() {
        // When a module that appears later in topo order depends on one that appears earlier,
        // it should be marked as Downward. When the reverse happens (earlier depends on later),
        // it should be marked as Upward.
        use std::path::PathBuf;

        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "test".to_string(),
            path: PathBuf::from("/test"),
        });
        // mod_a depends on mod_b (normal downward flow if a comes before b)
        let mod_a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx,
        });
        let mod_b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx,
        });
        graph.add_edge(crate_idx, mod_a, Edge::Contains);
        graph.add_edge(crate_idx, mod_b, Edge::Contains);
        // a -> b (a depends on b)
        graph.add_edge(
            mod_a,
            mod_b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        // There should be exactly one edge
        assert_eq!(ir.edges.len(), 1);
        let edge = &ir.edges[0];

        // The direction depends on topo order position
        // If from < to in layout order -> Downward
        // If from > to in layout order -> Upward
        assert!(
            matches!(edge.kind, EdgeKind::Downward | EdgeKind::Upward),
            "Edge should have direction: {:?}",
            edge.kind
        );
    }

    #[test]
    fn test_build_layout_multi_crate_grouping() {
        use std::path::PathBuf;

        // Simulate a workspace with 2 crates, each having 2 modules
        let mut graph = ArcGraph::new();

        // Crate A with modules a1, a2
        let crate_a = graph.add_node(Node::Crate {
            name: "crate_a".to_string(),
            path: PathBuf::from("/path/a"),
        });
        let mod_a1 = graph.add_node(Node::Module {
            name: "mod_a1".to_string(),
            crate_idx: crate_a,
        });
        let mod_a2 = graph.add_node(Node::Module {
            name: "mod_a2".to_string(),
            crate_idx: crate_a,
        });
        graph.add_edge(crate_a, mod_a1, Edge::Contains);
        graph.add_edge(crate_a, mod_a2, Edge::Contains);

        // Crate B with modules b1, b2
        let crate_b = graph.add_node(Node::Crate {
            name: "crate_b".to_string(),
            path: PathBuf::from("/path/b"),
        });
        let mod_b1 = graph.add_node(Node::Module {
            name: "mod_b1".to_string(),
            crate_idx: crate_b,
        });
        let mod_b2 = graph.add_node(Node::Module {
            name: "mod_b2".to_string(),
            crate_idx: crate_b,
        });
        graph.add_edge(crate_b, mod_b1, Edge::Contains);
        graph.add_edge(crate_b, mod_b2, Edge::Contains);

        // Crate A depends on Crate B
        graph.add_edge(
            crate_a,
            crate_b,
            Edge::CrateDep {
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        // Should have 6 items (2 crates + 4 modules)
        assert_eq!(ir.items.len(), 6);

        // Verify modules are grouped under their crates
        // Find positions
        let pos_crate_a = ir.items.iter().position(|i| i.label == "crate_a").unwrap();
        let pos_mod_a1 = ir.items.iter().position(|i| i.label == "mod_a1").unwrap();
        let pos_mod_a2 = ir.items.iter().position(|i| i.label == "mod_a2").unwrap();
        let pos_crate_b = ir.items.iter().position(|i| i.label == "crate_b").unwrap();
        let pos_mod_b1 = ir.items.iter().position(|i| i.label == "mod_b1").unwrap();
        let pos_mod_b2 = ir.items.iter().position(|i| i.label == "mod_b2").unwrap();

        // Crate A's modules should appear right after Crate A, before Crate B
        assert!(
            pos_crate_a < pos_mod_a1 && pos_mod_a1 < pos_crate_b,
            "mod_a1 should be between crate_a and crate_b"
        );
        assert!(
            pos_crate_a < pos_mod_a2 && pos_mod_a2 < pos_crate_b,
            "mod_a2 should be between crate_a and crate_b"
        );

        // Crate B's modules should appear after Crate B
        assert!(pos_crate_b < pos_mod_b1, "mod_b1 should be after crate_b");
        assert!(pos_crate_b < pos_mod_b2, "mod_b2 should be after crate_b");

        // CrateDep edge should be present (no ModuleDeps between crates)
        assert_eq!(
            ir.edges.len(),
            1,
            "Should have CrateDep edge (no ModuleDeps between crates)"
        );
        assert_eq!(ir.edges[0].from, pos_crate_a);
        assert_eq!(ir.edges[0].to, pos_crate_b);
    }

    // === Layout Item Tests ===

    #[test]
    fn test_layout_item_creation() {
        let crate_item = LayoutItem {
            id: 0,
            kind: ItemKind::Crate,
            label: "my_crate".to_string(),
            source_path: None,
            volatility: None,
        };
        let module_item = LayoutItem {
            id: 1,
            kind: ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            label: "my_module".to_string(),
            source_path: None,
            volatility: None,
        };
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
        let normal = LayoutEdge {
            from: 0,
            to: 1,
            kind: EdgeKind::Downward,
            source_locations: vec![],
            is_test: false,
        };
        let direct = LayoutEdge {
            from: 1,
            to: 0,
            kind: EdgeKind::DirectCycle,
            source_locations: vec![],
            is_test: false,
        };
        let trans = LayoutEdge {
            from: 2,
            to: 3,
            kind: EdgeKind::TransitiveCycle,
            source_locations: vec![],
            is_test: false,
        };

        assert_eq!(normal.from, 0);
        assert!(matches!(direct.kind, EdgeKind::DirectCycle));
        assert!(matches!(trans.kind, EdgeKind::TransitiveCycle));
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
        ir.add_edge(crate_id, mod_id, EdgeKind::Downward, vec![], false);

        assert_eq!(ir.items.len(), 2);
        assert_eq!(ir.edges.len(), 1);
        assert_eq!(ir.items[crate_id].label, "my_crate");
    }

    #[test]
    fn test_layout_item_default_source_path_is_none() {
        let mut ir = LayoutIR::new();
        let id = ir.add_item(ItemKind::Crate, "test".to_string());
        assert!(ir.items[id].source_path.is_none());
    }

    #[test]
    fn test_layout_item_default_volatility_is_none() {
        let mut ir = LayoutIR::new();
        let id = ir.add_item(ItemKind::Crate, "test".to_string());
        assert!(ir.items[id].volatility.is_none());
    }

    #[test]
    fn test_nested_module_hierarchy_ordering() {
        use std::path::PathBuf;

        // Setup: Crate mit nested Modulen
        // crate
        // ├── parent
        // │   ├── alpha_child
        // │   └── zebra_child
        // └── other_module (alphabetisch vor "parent", aber kein Kind)

        let mut graph = ArcGraph::new();

        let crate_idx = graph.add_node(Node::Crate {
            name: "test_crate".to_string(),
            path: PathBuf::from("/test"),
        });

        // Module absichtlich in "falscher" Reihenfolge hinzufügen
        let other = graph.add_node(Node::Module {
            name: "other_module".to_string(),
            crate_idx,
        });
        let zebra = graph.add_node(Node::Module {
            name: "zebra_child".to_string(),
            crate_idx,
        });
        let parent = graph.add_node(Node::Module {
            name: "parent".to_string(),
            crate_idx,
        });
        let alpha = graph.add_node(Node::Module {
            name: "alpha_child".to_string(),
            crate_idx,
        });

        // Hierarchie: crate → all modules, parent → children
        graph.add_edge(crate_idx, other, Edge::Contains);
        graph.add_edge(crate_idx, parent, Edge::Contains);
        graph.add_edge(parent, alpha, Edge::Contains);
        graph.add_edge(parent, zebra, Edge::Contains);

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        // Erwartete Reihenfolge:
        // 1. test_crate
        // 2. other_module (kein Kind, alphabetisch vor parent)
        // 3. parent
        // 4. alpha_child (Kind von parent, alphabetisch vor zebra)
        // 5. zebra_child (Kind von parent)

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();

        let pos_crate = labels.iter().position(|&l| l == "test_crate").unwrap();
        let pos_other = labels.iter().position(|&l| l == "other_module").unwrap();
        let pos_parent = labels.iter().position(|&l| l == "parent").unwrap();
        let pos_alpha = labels.iter().position(|&l| l == "alpha_child").unwrap();
        let pos_zebra = labels.iter().position(|&l| l == "zebra_child").unwrap();

        // Suppress unused variable warnings
        let _ = pos_crate;
        let _ = pos_other;

        // Kinder MÜSSEN direkt nach Parent kommen
        assert!(
            pos_alpha > pos_parent && pos_alpha < pos_parent + 3,
            "alpha_child must directly follow parent, not scattered. Labels: {:?}",
            labels
        );
        assert!(
            pos_zebra > pos_parent && pos_zebra < pos_parent + 3,
            "zebra_child must directly follow parent, not scattered. Labels: {:?}",
            labels
        );

        // Alphabetisch innerhalb Geschwister
        assert!(
            pos_alpha < pos_zebra,
            "alpha_child should come before zebra_child (alphabetical). Labels: {:?}",
            labels
        );
    }

    #[test]
    fn test_module_dependency_ordering() {
        use std::path::PathBuf;

        // Setup: 3 siblings with dependency chain
        // zebra -> beta -> alpha (zebra depends on beta, beta depends on alpha)
        // Alphabetical order: alpha, beta, zebra
        // Dependency order: zebra, beta, alpha (dependents first)

        let mut graph = ArcGraph::new();

        let crate_idx = graph.add_node(Node::Crate {
            name: "test_crate".to_string(),
            path: PathBuf::from("/test"),
        });

        // Add modules (order shouldn't matter due to sorting)
        let alpha = graph.add_node(Node::Module {
            name: "alpha".to_string(),
            crate_idx,
        });
        let beta = graph.add_node(Node::Module {
            name: "beta".to_string(),
            crate_idx,
        });
        let zebra = graph.add_node(Node::Module {
            name: "zebra".to_string(),
            crate_idx,
        });

        // Hierarchy: crate contains all modules
        graph.add_edge(crate_idx, alpha, Edge::Contains);
        graph.add_edge(crate_idx, beta, Edge::Contains);
        graph.add_edge(crate_idx, zebra, Edge::Contains);

        // Dependencies: zebra -> beta -> alpha
        graph.add_edge(
            zebra,
            beta,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            beta,
            alpha,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();

        let pos_alpha = labels.iter().position(|&l| l == "alpha").unwrap();
        let pos_beta = labels.iter().position(|&l| l == "beta").unwrap();
        let pos_zebra = labels.iter().position(|&l| l == "zebra").unwrap();

        // Dependency order: zebra first (depends on others), then beta, then alpha
        assert!(
            pos_zebra < pos_beta,
            "zebra should come before beta (zebra depends on beta). Labels: {:?}",
            labels
        );
        assert!(
            pos_beta < pos_alpha,
            "beta should come before alpha (beta depends on alpha). Labels: {:?}",
            labels
        );
    }

    #[test]
    fn test_test_edges_do_not_affect_sibling_sort_order() {
        use std::path::PathBuf;

        let mut graph = ArcGraph::new();

        let crate_idx = graph.add_node(Node::Crate {
            name: "my_crate".to_string(),
            path: PathBuf::from("/test"),
        });

        let alpha = graph.add_node(Node::Module {
            name: "alpha".to_string(),
            crate_idx,
        });
        let beta = graph.add_node(Node::Module {
            name: "beta".to_string(),
            crate_idx,
        });

        graph.add_edge(crate_idx, alpha, Edge::Contains);
        graph.add_edge(crate_idx, beta, Edge::Contains);

        // Production: alpha depends on beta → alpha should come first
        graph.add_edge(
            alpha,
            beta,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        // Test: beta depends on alpha (reverse direction) → must NOT affect order
        graph.add_edge(
            beta,
            alpha,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Test(crate::model::TestKind::Unit),
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();
        let pos_alpha = labels.iter().position(|&l| l == "alpha").unwrap();
        let pos_beta = labels.iter().position(|&l| l == "beta").unwrap();

        assert!(
            pos_alpha < pos_beta,
            "alpha should come before beta (production dep alpha→beta). \
             Test edge beta→alpha must not reverse this. Labels: {:?}",
            labels
        );
    }

    #[test]
    fn test_test_edges_do_not_affect_crate_sort_order() {
        use std::path::PathBuf;

        let mut graph = ArcGraph::new();

        let crate_a = graph.add_node(Node::Crate {
            name: "aaa".to_string(),
            path: PathBuf::from("/aaa"),
        });
        let crate_b = graph.add_node(Node::Crate {
            name: "bbb".to_string(),
            path: PathBuf::from("/bbb"),
        });

        // Production: crate_a depends on crate_b → crate_a first
        graph.add_edge(
            crate_a,
            crate_b,
            Edge::CrateDep {
                context: crate::model::EdgeContext::Production,
            },
        );
        // Test: crate_b depends on crate_a (reverse) → must NOT affect order
        graph.add_edge(
            crate_b,
            crate_a,
            Edge::CrateDep {
                context: crate::model::EdgeContext::Test(crate::model::TestKind::Unit),
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();
        let pos_a = labels.iter().position(|&l| l == "aaa").unwrap();
        let pos_b = labels.iter().position(|&l| l == "bbb").unwrap();

        assert!(
            pos_a < pos_b,
            "aaa should come before bbb (production dep aaa→bbb). \
             Test edge bbb→aaa must not reverse this. Labels: {:?}",
            labels
        );
    }

    #[test]
    fn test_crate_dep_suppressed_for_entry_point_module_dep() {
        use std::path::PathBuf;

        // Setup: Crate A has module mod_a that imports from Crate B's entry point.
        // This creates a ModuleDep from mod_a (Node::Module) to crate_b (Node::Crate).
        // The CrateDep between crate_a and crate_b should be suppressed.
        let mut graph = ArcGraph::new();

        let crate_a = graph.add_node(Node::Crate {
            name: "crate_a".to_string(),
            path: PathBuf::from("/path/a"),
        });
        let mod_a = graph.add_node(Node::Module {
            name: "mod_a".to_string(),
            crate_idx: crate_a,
        });
        let crate_b = graph.add_node(Node::Crate {
            name: "crate_b".to_string(),
            path: PathBuf::from("/path/b"),
        });

        // Hierarchy
        graph.add_edge(crate_a, mod_a, Edge::Contains);

        // CrateDep: crate_a -> crate_b
        graph.add_edge(
            crate_a,
            crate_b,
            Edge::CrateDep {
                context: crate::model::EdgeContext::Production,
            },
        );

        // ModuleDep: mod_a -> crate_b (entry-point import)
        graph.add_edge(
            mod_a,
            crate_b,
            Edge::ModuleDep {
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/mod_a.rs"),
                    line: 1,
                    symbols: vec!["Helper".to_string()],
                    module_path: "crate_b".to_string(),
                }],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        // Should have 3 items (2 crates + 1 module)
        assert_eq!(ir.items.len(), 3);

        // CrateDep should be suppressed — only the ModuleDep edge should remain
        assert_eq!(
            ir.edges.len(),
            1,
            "CrateDep should be suppressed when ModuleDep exists. Edges: {:?}",
            ir.edges
                .iter()
                .map(|e| format!(
                    "{}->{} locs={}",
                    ir.items[e.from].label,
                    ir.items[e.to].label,
                    e.source_locations.len()
                ))
                .collect::<Vec<_>>()
        );

        // The remaining edge should be the ModuleDep (has source_locations)
        assert!(
            !ir.edges[0].source_locations.is_empty(),
            "Remaining edge should be ModuleDep with source_locations"
        );
    }

    #[test]
    fn test_crate_dep_suppressed_for_crate_to_crate_module_dep() {
        use std::path::PathBuf;

        // Setup: Crate A's root (lib.rs) imports from Crate B's entry point.
        // This creates a ModuleDep from crate_a (Node::Crate) to crate_b (Node::Crate).
        // The CrateDep between crate_a and crate_b should be suppressed.
        let mut graph = ArcGraph::new();

        let crate_a = graph.add_node(Node::Crate {
            name: "crate_a".to_string(),
            path: PathBuf::from("/path/a"),
        });
        let crate_b = graph.add_node(Node::Crate {
            name: "crate_b".to_string(),
            path: PathBuf::from("/path/b"),
        });

        // CrateDep: crate_a -> crate_b
        graph.add_edge(
            crate_a,
            crate_b,
            Edge::CrateDep {
                context: crate::model::EdgeContext::Production,
            },
        );

        // ModuleDep: crate_a -> crate_b (root-to-entry-point)
        graph.add_edge(
            crate_a,
            crate_b,
            Edge::ModuleDep {
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/lib.rs"),
                    line: 3,
                    symbols: vec!["Config".to_string()],
                    module_path: "crate_b".to_string(),
                }],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        // CrateDep should be suppressed
        assert_eq!(
            ir.edges.len(),
            1,
            "CrateDep should be suppressed for Crate→Crate ModuleDep"
        );
        assert!(
            !ir.edges[0].source_locations.is_empty(),
            "Remaining edge should be ModuleDep"
        );
    }

    #[test]
    fn test_crate_dep_suppressed_for_crate_to_module_module_dep() {
        use std::path::PathBuf;

        // Setup: Crate A's root imports from Crate B's module (not entry point).
        // ModuleDep from crate_a (Node::Crate) to mod_b (Node::Module).
        // CrateDep should be suppressed.
        let mut graph = ArcGraph::new();

        let crate_a = graph.add_node(Node::Crate {
            name: "crate_a".to_string(),
            path: PathBuf::from("/path/a"),
        });
        let crate_b = graph.add_node(Node::Crate {
            name: "crate_b".to_string(),
            path: PathBuf::from("/path/b"),
        });
        let mod_b = graph.add_node(Node::Module {
            name: "mod_b".to_string(),
            crate_idx: crate_b,
        });

        // Hierarchy
        graph.add_edge(crate_b, mod_b, Edge::Contains);

        // CrateDep: crate_a -> crate_b
        graph.add_edge(
            crate_a,
            crate_b,
            Edge::CrateDep {
                context: crate::model::EdgeContext::Production,
            },
        );

        // ModuleDep: crate_a -> mod_b (root imports from module)
        graph.add_edge(
            crate_a,
            mod_b,
            Edge::ModuleDep {
                locations: vec![SourceLocation {
                    file: PathBuf::from("src/lib.rs"),
                    line: 5,
                    symbols: vec!["parse".to_string()],
                    module_path: "mod_b".to_string(),
                }],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        // CrateDep should be suppressed
        assert_eq!(
            ir.edges.len(),
            1,
            "CrateDep should be suppressed for Crate→Module ModuleDep"
        );
        assert!(
            !ir.edges[0].source_locations.is_empty(),
            "Remaining edge should be ModuleDep"
        );
    }

    #[test]
    fn test_subtree_dependency_ordering() {
        use std::path::PathBuf;

        // Setup: Two parent modules, each with a child.
        // parent_a::child_a depends on parent_b::child_b
        // Expected: parent_a before parent_b (subtree dependency aggregation)
        //
        // crate
        // ├── parent_a         <-- should come FIRST (its subtree depends on parent_b's subtree)
        // │   └── child_a      <-- depends on child_b
        // └── parent_b         <-- should come SECOND (dependency target)
        //     └── child_b      <-- used by child_a

        let mut graph = ArcGraph::new();

        let crate_idx = graph.add_node(Node::Crate {
            name: "test_crate".to_string(),
            path: PathBuf::from("/test"),
        });

        // Create parents (alphabetical: parent_a before parent_b)
        let parent_a = graph.add_node(Node::Module {
            name: "parent_a".to_string(),
            crate_idx,
        });
        let parent_b = graph.add_node(Node::Module {
            name: "parent_b".to_string(),
            crate_idx,
        });

        // Create children
        let child_a = graph.add_node(Node::Module {
            name: "child_a".to_string(),
            crate_idx,
        });
        let child_b = graph.add_node(Node::Module {
            name: "child_b".to_string(),
            crate_idx,
        });

        // Hierarchy: crate → parents, parents → children
        graph.add_edge(crate_idx, parent_a, Edge::Contains);
        graph.add_edge(crate_idx, parent_b, Edge::Contains);
        graph.add_edge(parent_a, child_a, Edge::Contains);
        graph.add_edge(parent_b, child_b, Edge::Contains);

        // Cross-subtree dependency: child_a -> child_b
        // This means parent_a's subtree depends on parent_b's subtree
        graph.add_edge(
            child_a,
            child_b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();

        let pos_parent_a = labels.iter().position(|&l| l == "parent_a").unwrap();
        let pos_parent_b = labels.iter().position(|&l| l == "parent_b").unwrap();

        // parent_a should come before parent_b because parent_a's subtree
        // depends on parent_b's subtree (child_a -> child_b)
        assert!(
            pos_parent_a < pos_parent_b,
            "parent_a should come before parent_b (subtree dependency). Labels: {:?}",
            labels
        );
    }

    #[test]
    fn test_crate_order_respects_inter_crate_module_deps() {
        use std::path::PathBuf;

        // When modules in crate_a depend on modules in crate_b
        // but there is NO CrateDep edge, the crate ordering should still
        // place crate_a before crate_b (dependent crate first).
        let mut graph = ArcGraph::new();

        // crate_b alphabetically before crate_a — exposes the bug
        let crate_b = graph.add_node(Node::Crate {
            name: "crate_b".to_string(),
            path: PathBuf::from("/b"),
        });
        let crate_a = graph.add_node(Node::Crate {
            name: "crate_a".to_string(),
            path: PathBuf::from("/a"),
        });

        let mod_a = graph.add_node(Node::Module {
            name: "mod_a".to_string(),
            crate_idx: crate_a,
        });
        let mod_b = graph.add_node(Node::Module {
            name: "mod_b".to_string(),
            crate_idx: crate_b,
        });

        // Hierarchy
        graph.add_edge(crate_a, mod_a, Edge::Contains);
        graph.add_edge(crate_b, mod_b, Edge::Contains);

        // Module dependency: mod_a -> mod_b (crate_a depends on crate_b)
        // but NO CrateDep edge!
        graph.add_edge(
            mod_a,
            mod_b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();

        let pos_crate_a = labels.iter().position(|&l| l == "crate_a").unwrap();
        let pos_crate_b = labels.iter().position(|&l| l == "crate_b").unwrap();

        // crate_a depends on crate_b (via module dep) → crate_a should come first
        assert!(
            pos_crate_a < pos_crate_b,
            "crate_a should come before crate_b (inter-crate module dep). Labels: {:?}",
            labels
        );

        // The ModuleDep edge should be Downward (not Upward)
        let mod_a_to_mod_b = ir.edges.iter().find(|e| {
            let from_label = &ir.items[e.from].label;
            let to_label = &ir.items[e.to].label;
            from_label == "mod_a" && to_label == "mod_b"
        });
        assert!(
            mod_a_to_mod_b.is_some(),
            "Should have edge from mod_a to mod_b"
        );
        assert_eq!(
            mod_a_to_mod_b.unwrap().kind,
            EdgeKind::Downward,
            "Inter-crate module dep should be Downward, not Upward"
        );
    }

    // === stable_toposort Cycle Tests ===

    #[test]
    fn test_stable_toposort_with_cycle_returns_sorted() {
        use std::path::PathBuf;

        // Build a graph with a cycle (A⇄B) — stable_toposort should return
        // non-empty result via SCC fallback instead of vec![]
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "test".to_string(),
            path: PathBuf::from("/test"),
        });
        let a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx,
        });
        let b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx,
        });

        // Build sibling_deps DiGraph with cycle
        let mut sibling_deps: DiGraph<NodeIndex, usize> = DiGraph::new();
        let sa = sibling_deps.add_node(a);
        let sb = sibling_deps.add_node(b);
        sibling_deps.add_edge(sa, sb, 1);
        sibling_deps.add_edge(sb, sa, 1); // cycle

        let children = vec![a, b];
        let result = stable_toposort(&sibling_deps, &children, |idx| match &graph[idx] {
            Node::Module { name, .. } => name.clone(),
            Node::Crate { name, .. } => name.clone(),
        });

        assert_eq!(result.len(), 2, "Should return all nodes, not empty vec");
        // Both nodes present
        assert!(result.contains(&a), "Should contain a");
        assert!(result.contains(&b), "Should contain b");
        // Alphabetical within SCC
        let pos_a = result.iter().position(|&n| n == a).unwrap();
        let pos_b = result.iter().position(|&n| n == b).unwrap();
        assert!(pos_a < pos_b, "a before b (alphabetical in SCC)");
    }

    // === SCC Fallback Tests ===

    #[test]
    fn test_scc_fallback_three_node_cycle() {
        // Pure cycle: A→B→C→A — all nodes in one SCC
        // Expected: alphabetical order [A, B, C]
        let mut graph: DiGraph<NodeIndex, usize> = DiGraph::new();
        let a_orig = NodeIndex::new(10);
        let b_orig = NodeIndex::new(11);
        let c_orig = NodeIndex::new(12);
        let a = graph.add_node(a_orig);
        let b = graph.add_node(b_orig);
        let c = graph.add_node(c_orig);
        graph.add_edge(a, b, 1);
        graph.add_edge(b, c, 1);
        graph.add_edge(c, a, 1);

        let node_to_orig: HashMap<petgraph::graph::NodeIndex, NodeIndex> =
            graph.node_indices().map(|n| (n, graph[n])).collect();

        let get_name = |idx: NodeIndex| -> String {
            match idx.index() {
                10 => "a".to_string(),
                11 => "b".to_string(),
                12 => "c".to_string(),
                _ => panic!("unexpected index"),
            }
        };

        let result = scc_fallback_sort(&graph, &node_to_orig, &get_name);
        let names: Vec<String> = result.iter().map(|&idx| get_name(idx)).collect();
        assert_eq!(
            names,
            vec!["a", "b", "c"],
            "SCC members sorted alphabetically"
        );
    }

    #[test]
    fn test_scc_fallback_with_external_node() {
        // SCC {A,B} (A⇄B) + standalone C, C→A edge
        // Condensed: SCC{A,B} → C (C depends on SCC)
        // Expected: [A, B, C] — SCC first (expanded alphabetically), C after
        let mut graph: DiGraph<NodeIndex, usize> = DiGraph::new();
        let a_orig = NodeIndex::new(10);
        let b_orig = NodeIndex::new(11);
        let c_orig = NodeIndex::new(12);
        let a = graph.add_node(a_orig);
        let b = graph.add_node(b_orig);
        let c = graph.add_node(c_orig);
        graph.add_edge(a, b, 1);
        graph.add_edge(b, a, 1); // cycle A⇄B
        graph.add_edge(c, a, 1); // C depends on A

        let node_to_orig: HashMap<petgraph::graph::NodeIndex, NodeIndex> =
            graph.node_indices().map(|n| (n, graph[n])).collect();

        let get_name = |idx: NodeIndex| -> String {
            match idx.index() {
                10 => "a".to_string(),
                11 => "b".to_string(),
                12 => "c".to_string(),
                _ => panic!("unexpected index"),
            }
        };

        let result = scc_fallback_sort(&graph, &node_to_orig, &get_name);
        let names: Vec<String> = result.iter().map(|&idx| get_name(idx)).collect();
        // C depends on SCC{A,B}, so C comes first (dependent before dependency)
        // Wait — in our convention: edge C→A means C depends on A.
        // toposort: dependents come before dependencies → C first, then SCC{A,B}
        assert_eq!(
            names,
            vec!["c", "a", "b"],
            "Dependent C first, then SCC{{A,B}} alphabetically"
        );
    }

    // === Cross-Subtree Cycle Tests (ca-0211) ===

    #[test]
    fn test_cross_subtree_cycles_weighted_asymmetric() {
        use std::path::PathBuf;

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

        let mut graph = ArcGraph::new();

        let crate_idx = graph.add_node(Node::Crate {
            name: "test_crate".to_string(),
            path: PathBuf::from("/test"),
        });

        // Top-level modules (groups)
        let mod_a = graph.add_node(Node::Module {
            name: "a".to_string(),
            crate_idx,
        });
        let mod_b = graph.add_node(Node::Module {
            name: "b".to_string(),
            crate_idx,
        });
        let mod_c = graph.add_node(Node::Module {
            name: "c".to_string(),
            crate_idx,
        });
        let mod_d = graph.add_node(Node::Module {
            name: "d".to_string(),
            crate_idx,
        });

        // Sub-modules
        let _a1 = graph.add_node(Node::Module {
            name: "a1".to_string(),
            crate_idx,
        });
        let _a2 = graph.add_node(Node::Module {
            name: "a2".to_string(),
            crate_idx,
        });
        let _a3 = graph.add_node(Node::Module {
            name: "a3".to_string(),
            crate_idx,
        });
        let b1 = graph.add_node(Node::Module {
            name: "b1".to_string(),
            crate_idx,
        });
        let b2 = graph.add_node(Node::Module {
            name: "b2".to_string(),
            crate_idx,
        });
        let d1 = graph.add_node(Node::Module {
            name: "d1".to_string(),
            crate_idx,
        });
        let d2 = graph.add_node(Node::Module {
            name: "d2".to_string(),
            crate_idx,
        });
        let d3 = graph.add_node(Node::Module {
            name: "d3".to_string(),
            crate_idx,
        });

        // Hierarchy: crate -> groups -> sub-modules
        graph.add_edge(crate_idx, mod_a, Edge::Contains);
        graph.add_edge(crate_idx, mod_b, Edge::Contains);
        graph.add_edge(crate_idx, mod_c, Edge::Contains);
        graph.add_edge(crate_idx, mod_d, Edge::Contains);

        graph.add_edge(mod_a, _a1, Edge::Contains);
        graph.add_edge(mod_a, _a2, Edge::Contains);
        graph.add_edge(mod_a, _a3, Edge::Contains);
        graph.add_edge(mod_b, b1, Edge::Contains);
        graph.add_edge(mod_b, b2, Edge::Contains);
        graph.add_edge(mod_d, d1, Edge::Contains);
        graph.add_edge(mod_d, d2, Edge::Contains);
        graph.add_edge(mod_d, d3, Edge::Contains);

        // Cycle 1: B1 <-> C
        graph.add_edge(
            b1,
            mod_c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            mod_c,
            b1,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        // Cycle 2: B1 <-> D2
        graph.add_edge(
            b1,
            d2,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            d2,
            b1,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        // Asymmetric non-cycle edges: D's subtree uses B and C
        graph.add_edge(
            d1,
            b2,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            d3,
            mod_c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles = vec![];
        let ir = build_layout(&graph, &cycles);

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
    fn test_barycenter_incoming_gives_dependents() {
        // Verify edge direction: A→B means "A depends on B"
        // neighbors_directed(B, Incoming) should return A (the dependent)
        let mut g: DiGraph<NodeIndex, usize> = DiGraph::new();
        let a_orig = NodeIndex::new(0);
        let b_orig = NodeIndex::new(1);
        let a = g.add_node(a_orig);
        let b = g.add_node(b_orig);
        g.add_edge(a, b, 1); // A depends on B

        let incoming: Vec<_> = g
            .neighbors_directed(b, petgraph::Direction::Incoming)
            .collect();
        assert_eq!(incoming, vec![a], "Incoming neighbors of B should be [A]");
    }

    #[test]
    fn test_barycenter_symmetric_diamond() {
        use std::path::PathBuf;

        // Symmetric diamond: A→C, A→D, B→C, B→D
        // Barycenter scores identical → alphabetical fallback → A, B, C, D
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "test".to_string(),
            path: PathBuf::from("/test"),
        });
        let a = graph.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let b = graph.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        let c = graph.add_node(Node::Module {
            name: "c".into(),
            crate_idx,
        });
        let d = graph.add_node(Node::Module {
            name: "d".into(),
            crate_idx,
        });

        graph.add_edge(crate_idx, a, Edge::Contains);
        graph.add_edge(crate_idx, b, Edge::Contains);
        graph.add_edge(crate_idx, c, Edge::Contains);
        graph.add_edge(crate_idx, d, Edge::Contains);

        // A→C, A→D (A depends on C and D)
        graph.add_edge(
            a,
            c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            a,
            d,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        // B→C, B→D (B depends on C and D)
        graph.add_edge(
            b,
            c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            d,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();
        let pos_a = labels.iter().position(|&l| l == "a").unwrap();
        let pos_b = labels.iter().position(|&l| l == "b").unwrap();
        let pos_c = labels.iter().position(|&l| l == "c").unwrap();
        let pos_d = labels.iter().position(|&l| l == "d").unwrap();

        // Symmetric: A and B before C and D, alphabetical within each group
        assert!(pos_a < pos_c, "A before C. Labels: {labels:?}");
        assert!(pos_b < pos_d, "B before D. Labels: {labels:?}");
        assert!(
            pos_a < pos_b,
            "A before B (alphabetical). Labels: {labels:?}"
        );
    }

    #[test]
    fn test_barycenter_reduces_crossings() {
        use std::path::PathBuf;

        // Graph: A→D, B→C (A depends on D, B depends on C)
        // Alphabetical: A(0), B(1), C(2), D(3) — Arc A→D crosses Arc B→C
        // Barycenter: A(0), B(1), D(2) [score=0.0 from A@0], C(3) [score=1.0 from B@1]
        // D should come before C to minimize crossings
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "test".to_string(),
            path: PathBuf::from("/test"),
        });
        let a = graph.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let b = graph.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        let c = graph.add_node(Node::Module {
            name: "c".into(),
            crate_idx,
        });
        let d = graph.add_node(Node::Module {
            name: "d".into(),
            crate_idx,
        });

        graph.add_edge(crate_idx, a, Edge::Contains);
        graph.add_edge(crate_idx, b, Edge::Contains);
        graph.add_edge(crate_idx, c, Edge::Contains);
        graph.add_edge(crate_idx, d, Edge::Contains);

        // A depends on D, B depends on C
        graph.add_edge(
            a,
            d,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();
        let pos_c = labels.iter().position(|&l| l == "c").unwrap();
        let pos_d = labels.iter().position(|&l| l == "d").unwrap();

        // D should come before C (barycenter: D's dependent A is at pos 0, C's dependent B at pos 1)
        assert!(
            pos_d < pos_c,
            "D should come before C (barycenter reduces crossings). Labels: {labels:?}"
        );
    }

    #[test]
    fn test_barycenter_linear_chain_unchanged() {
        use std::path::PathBuf;

        // Linear chain: A→B→C — should produce same order as alphabetical
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "test".to_string(),
            path: PathBuf::from("/test"),
        });
        let a = graph.add_node(Node::Module {
            name: "a".into(),
            crate_idx,
        });
        let b = graph.add_node(Node::Module {
            name: "b".into(),
            crate_idx,
        });
        let c = graph.add_node(Node::Module {
            name: "c".into(),
            crate_idx,
        });

        graph.add_edge(crate_idx, a, Edge::Contains);
        graph.add_edge(crate_idx, b, Edge::Contains);
        graph.add_edge(crate_idx, c, Edge::Contains);

        graph.add_edge(
            a,
            b,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );
        graph.add_edge(
            b,
            c,
            Edge::ModuleDep {
                locations: vec![],
                context: crate::model::EdgeContext::Production,
            },
        );

        let cycles: Vec<Cycle> = vec![];
        let ir = build_layout(&graph, &cycles);

        let labels: Vec<&str> = ir.items.iter().map(|i| i.label.as_str()).collect();
        let pos_a = labels.iter().position(|&l| l == "a").unwrap();
        let pos_b = labels.iter().position(|&l| l == "b").unwrap();
        let pos_c = labels.iter().position(|&l| l == "c").unwrap();

        assert!(pos_a < pos_b, "A before B. Labels: {labels:?}");
        assert!(pos_b < pos_c, "B before C. Labels: {labels:?}");
    }
}
