//! Layout IR & Algorithms

use crate::graph::{ArcGraph, Edge};
use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

/// Index into LayoutIR.items
pub type NodeId = usize;

/// A cycle in the module dependency graph (SCC with >1 node)
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    /// NodeIndices participating in this cycle
    pub nodes: Vec<NodeIndex>,
}

/// Topologically sort graph nodes, treating cycle members as a unit.
/// Only considers CrateDep and ModuleDep edges (ignores Contains edges).
/// Cycle nodes are sorted alphabetically within their group.
pub fn topo_sort(graph: &ArcGraph, cycles: &[Cycle]) -> Vec<NodeIndex> {
    use petgraph::algo::toposort;
    use petgraph::graph::DiGraph;
    use std::collections::{HashMap, HashSet};

    // Build set of all cycle members for quick lookup
    let cycle_members: HashSet<NodeIndex> = cycles
        .iter()
        .flat_map(|c| c.nodes.iter().copied())
        .collect();

    // Map each node to its "representative" (itself, or first cycle member)
    let mut node_to_rep: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    for idx in graph.node_indices() {
        if cycle_members.contains(&idx) {
            // Find which cycle this node belongs to
            for cycle in cycles {
                if cycle.nodes.contains(&idx) {
                    // Use first node in cycle as representative
                    node_to_rep.insert(idx, cycle.nodes[0]);
                    break;
                }
            }
        } else {
            node_to_rep.insert(idx, idx);
        }
    }

    // Build condensed graph with only dependency edges
    let mut condensed: DiGraph<NodeIndex, ()> = DiGraph::new();
    let mut rep_to_condensed: HashMap<NodeIndex, petgraph::graph::NodeIndex> = HashMap::new();

    // Add nodes (one per representative)
    for &rep in node_to_rep.values().collect::<HashSet<_>>() {
        let cond_idx = condensed.add_node(rep);
        rep_to_condensed.insert(rep, cond_idx);
    }

    // Add edges (only CrateDep and ModuleDep, mapped to representatives)
    for edge_idx in graph.edge_indices() {
        match graph[edge_idx] {
            Edge::CrateDep | Edge::ModuleDep(_) => {
                let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
                let src_rep = node_to_rep[&src];
                let dst_rep = node_to_rep[&dst];
                // Skip self-loops (edges within same cycle)
                if src_rep != dst_rep {
                    let src_cond = rep_to_condensed[&src_rep];
                    let dst_cond = rep_to_condensed[&dst_rep];
                    // Avoid duplicate edges
                    if !condensed.contains_edge(src_cond, dst_cond) {
                        condensed.add_edge(src_cond, dst_cond, ());
                    }
                }
            }
            Edge::Contains => {} // Ignore hierarchy edges
        }
    }

    // Topological sort on condensed graph (dependents first: modules that depend on others come first)
    let sorted_reps: Vec<_> = match toposort(&condensed, None) {
        Ok(order) => order, // No .rev() - dependents appear before their dependencies
        Err(_) => {
            // Cycle in condensed graph shouldn't happen, but fallback to node order
            condensed.node_indices().collect()
        }
    };

    // Helper to get node name for sorting
    let node_name = |idx: NodeIndex| -> String {
        match &graph[idx] {
            crate::graph::Node::Crate { name, .. } => name.clone(),
            crate::graph::Node::Module { name, .. } => name.clone(),
        }
    };

    // Expand representatives back to original nodes
    let mut result = Vec::new();
    for cond_idx in sorted_reps {
        let rep = condensed[cond_idx];
        if cycle_members.contains(&rep) {
            // Find the cycle and add all members sorted alphabetically
            for cycle in cycles {
                if cycle.nodes.contains(&rep) {
                    let mut members = cycle.nodes.clone();
                    members.sort_by_key(|a| node_name(*a));
                    result.extend(members);
                    break;
                }
            }
        } else {
            result.push(rep);
        }
    }

    result
}

/// Hierarchically sorted modules for a parent, collecting children recursively.
/// Children are sorted alphabetically within each level.
fn collect_children_recursive(
    parent_idx: NodeIndex,
    graph: &ArcGraph,
    module_indices: &[NodeIndex],
    added: &mut std::collections::HashSet<NodeIndex>,
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

    // Sort children alphabetically (tie-breaker within same hierarchy level)
    children.sort_by_key(|&idx| {
        if let Node::Module { name, .. } = &graph[idx] {
            name.clone()
        } else {
            String::new()
        }
    });

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

/// Build LayoutIR from graph, sorted order, and cycle information.
/// Converts graph nodes to LayoutItems with proper nesting and edges with cycle markers.
pub fn build_layout(graph: &ArcGraph, order: &[NodeIndex], cycles: &[Cycle]) -> LayoutIR {
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
    let (crate_indices, module_indices): (Vec<NodeIndex>, Vec<NodeIndex>) = order
        .iter()
        .partition(|&idx| matches!(graph[*idx], Node::Crate { .. }));

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
    }

    // Add dependency edges (CrateDep and ModuleDep only)
    for edge_idx in graph.edge_indices() {
        match &graph[edge_idx] {
            Edge::CrateDep => {
                let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
                if let (Some(&from), Some(&to)) = (node_map.get(&src), node_map.get(&dst)) {
                    let kind = if cycle_pairs.contains(&(src, dst)) {
                        EdgeKind::TransitiveCycle
                    } else {
                        EdgeKind::Normal
                    };
                    ir.add_edge(from, to, kind, vec![]);
                }
            }
            Edge::ModuleDep(locations) => {
                let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
                if let (Some(&from), Some(&to)) = (node_map.get(&src), node_map.get(&dst)) {
                    let kind = if cycle_pairs.contains(&(src, dst)) {
                        // Check if it's a direct cycle (A->B and B->A both exist)
                        if cycle_pairs.contains(&(dst, src))
                            && graph.contains_edge(dst, src)
                            && matches!(
                                graph[graph.find_edge(dst, src).unwrap()],
                                Edge::ModuleDep(_)
                            )
                        {
                            EdgeKind::DirectCycle
                        } else {
                            EdgeKind::TransitiveCycle
                        }
                    } else {
                        EdgeKind::Normal
                    };
                    ir.add_edge(from, to, kind, locations.clone());
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

    // Copy only ModuleDep edges
    for edge_idx in graph.edge_indices() {
        if matches!(graph[edge_idx], Edge::ModuleDep(_)) {
            let (src, dst) = graph.edge_endpoints(edge_idx).unwrap();
            filtered.add_edge(node_map[&src], node_map[&dst], Edge::ModuleDep(vec![]));
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeKind {
    Normal,
    DirectCycle,
    TransitiveCycle,
}

use crate::graph::SourceLocation;

#[derive(Debug, Clone)]
pub struct LayoutEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub source_locations: Vec<SourceLocation>,
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
        self.items.push(LayoutItem { id, kind, label });
        id
    }

    pub fn add_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
        source_locations: Vec<SourceLocation>,
    ) {
        self.edges.push(LayoutEdge {
            from,
            to,
            kind,
            source_locations,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ArcGraph, Edge, Node, SourceLocation};
    use petgraph::graph::NodeIndex;
    use std::path::PathBuf;

    #[test]
    fn test_layout_edge_has_source_locations() {
        let edge = LayoutEdge {
            from: 0,
            to: 1,
            kind: EdgeKind::Normal,
            source_locations: vec![SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 42,
            }],
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
        graph.add_edge(a, b, Edge::ModuleDep(vec![]));
        graph.add_edge(b, c, Edge::ModuleDep(vec![]));

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
        graph.add_edge(a, b, Edge::ModuleDep(vec![]));
        graph.add_edge(b, a, Edge::ModuleDep(vec![]));

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
        graph.add_edge(a, b, Edge::ModuleDep(vec![]));
        graph.add_edge(b, c, Edge::ModuleDep(vec![]));
        graph.add_edge(c, a, Edge::ModuleDep(vec![]));

        let cycles = detect_cycles(&graph);
        assert_eq!(cycles.len(), 1, "Should detect one cycle");
        assert_eq!(cycles[0].nodes.len(), 3, "Cycle should contain 3 nodes");
    }

    // === Topological Sort Tests ===

    #[test]
    fn test_topo_sort_simple() {
        // A -> B -> C (A depends on B, B depends on C)
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
        graph.add_edge(a, b, Edge::ModuleDep(vec![]));
        graph.add_edge(b, c, Edge::ModuleDep(vec![]));

        let cycles: Vec<Cycle> = vec![];
        let sorted = topo_sort(&graph, &cycles);

        // All nodes should be in result
        assert_eq!(sorted.len(), 3);

        // A should come before B, B before C (dependents first, dependencies below)
        let pos_a = sorted.iter().position(|&n| n == a).unwrap();
        let pos_b = sorted.iter().position(|&n| n == b).unwrap();
        let pos_c = sorted.iter().position(|&n| n == c).unwrap();
        assert!(pos_a < pos_b, "A should come before B (A depends on B)");
        assert!(pos_b < pos_c, "B should come before C (B depends on C)");
    }

    #[test]
    fn test_topo_sort_with_cycles() {
        // D -> A <-> B -> C
        // D depends on cycle {A,B}, B depends on C
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
        graph.add_edge(d, a, Edge::ModuleDep(vec![]));
        graph.add_edge(a, b, Edge::ModuleDep(vec![]));
        graph.add_edge(b, a, Edge::ModuleDep(vec![])); // cycle
        graph.add_edge(b, c, Edge::ModuleDep(vec![]));

        let cycles = vec![Cycle { nodes: vec![a, b] }];
        let sorted = topo_sort(&graph, &cycles);

        // All 4 nodes should be present
        assert_eq!(sorted.len(), 4);

        // D depends on cycle, so D comes first (dependents before dependencies)
        // Cycle members (A, B) depend on C, so cycle comes before C
        let pos_a = sorted.iter().position(|&n| n == a).unwrap();
        let pos_b = sorted.iter().position(|&n| n == b).unwrap();
        let pos_c = sorted.iter().position(|&n| n == c).unwrap();
        let pos_d = sorted.iter().position(|&n| n == d).unwrap();

        assert!(
            pos_d < pos_a && pos_d < pos_b,
            "D should come before cycle (D depends on cycle)"
        );
        assert!(
            pos_a < pos_c && pos_b < pos_c,
            "Cycle should come before C (cycle depends on C)"
        );

        // Within cycle, alphabetical order: A before B
        assert!(pos_a < pos_b, "Within cycle: A before B (alphabetical)");
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
        graph.add_edge(mod_a, mod_b, Edge::ModuleDep(vec![]));

        let cycles: Vec<Cycle> = vec![];
        let order = topo_sort(&graph, &cycles);
        let ir = build_layout(&graph, &order, &cycles);

        // Should have 3 items (1 crate + 2 modules)
        assert_eq!(ir.items.len(), 3);

        // Should have 1 dependency edge (mod_a -> mod_b)
        assert_eq!(ir.edges.len(), 1);
        assert!(matches!(ir.edges[0].kind, EdgeKind::Normal));
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
        graph.add_edge(a, b, Edge::ModuleDep(vec![]));
        graph.add_edge(b, a, Edge::ModuleDep(vec![])); // cycle

        let cycles = detect_cycles(&graph);
        let order = topo_sort(&graph, &cycles);
        let ir = build_layout(&graph, &order, &cycles);

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
        graph.add_edge(crate_a, crate_b, Edge::CrateDep);

        let cycles: Vec<Cycle> = vec![];
        let order = topo_sort(&graph, &cycles);
        let ir = build_layout(&graph, &order, &cycles);

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
    }

    // === Layout Item Tests ===

    #[test]
    fn test_layout_item_creation() {
        let crate_item = LayoutItem {
            id: 0,
            kind: ItemKind::Crate,
            label: "my_crate".to_string(),
        };
        let module_item = LayoutItem {
            id: 1,
            kind: ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            label: "my_module".to_string(),
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
            kind: EdgeKind::Normal,
            source_locations: vec![],
        };
        let direct = LayoutEdge {
            from: 1,
            to: 0,
            kind: EdgeKind::DirectCycle,
            source_locations: vec![],
        };
        let trans = LayoutEdge {
            from: 2,
            to: 3,
            kind: EdgeKind::TransitiveCycle,
            source_locations: vec![],
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
        ir.add_edge(crate_id, mod_id, EdgeKind::Normal, vec![]);

        assert_eq!(ir.items.len(), 2);
        assert_eq!(ir.edges.len(), 1);
        assert_eq!(ir.items[crate_id].label, "my_crate");
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
        let order = topo_sort(&graph, &cycles);
        let ir = build_layout(&graph, &order, &cycles);

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
}
