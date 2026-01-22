//! Graph Types & Builder

use crate::analyze::{CrateInfo, DependencyRef, ModuleInfo, ModuleTree};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Node {
    Crate { name: String, path: PathBuf },
    Module { name: String, crate_idx: NodeIndex },
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: usize,
}

pub enum Edge {
    CrateDep,
    ModuleDep(Vec<SourceLocation>),
    Contains,
}

pub type ArcGraph = DiGraph<Node, Edge>;

/// Build a unified graph from crate and module analysis data.
pub fn build_graph(crates: &[CrateInfo], modules: &[ModuleTree]) -> ArcGraph {
    let mut graph = DiGraph::new();
    let mut crate_map: HashMap<String, NodeIndex> = HashMap::new();
    let mut module_map: HashMap<String, NodeIndex> = HashMap::new();
    let mut module_deps: Vec<(String, Vec<DependencyRef>)> = Vec::new();

    // Phase 1: Add all Crate nodes
    for crate_info in crates {
        let idx = graph.add_node(Node::Crate {
            name: crate_info.name.clone(),
            path: crate_info.path.clone(),
        });
        crate_map.insert(crate_info.name.clone(), idx);
    }

    // Phase 2: Add all Module nodes and Contains edges
    for module_tree in modules {
        // Match module tree to crate by name (root module name matches crate name with - -> _)
        let crate_name_normalized = module_tree.root.name.replace('_', "-");
        let crate_idx = crate_map
            .get(&module_tree.root.name)
            .or_else(|| crate_map.get(&crate_name_normalized));

        if let Some(&crate_idx) = crate_idx {
            // Add child modules (skip root since it's the crate itself)
            for child in &module_tree.root.children {
                add_modules_recursive(
                    &mut graph,
                    child,
                    crate_idx,
                    crate_idx,
                    &mut module_map,
                    &mut module_deps,
                );
            }
        }
    }

    // Phase 3: Add CrateDep edges
    for crate_info in crates {
        if let Some(&from_idx) = crate_map.get(&crate_info.name) {
            for dep_name in &crate_info.dependencies {
                if let Some(&to_idx) = crate_map.get(dep_name) {
                    graph.add_edge(from_idx, to_idx, Edge::CrateDep);
                }
            }
        }
    }

    // Phase 4: Add ModuleDep edges
    for (from_path, deps) in &module_deps {
        if let Some(&from_idx) = module_map.get(from_path) {
            for dep in deps {
                if let Some(&to_idx) = module_map.get(&dep.module_target()) {
                    let locations = vec![SourceLocation {
                        file: dep.source_file.clone(),
                        line: dep.line,
                    }];
                    graph.add_edge(from_idx, to_idx, Edge::ModuleDep(locations));
                }
            }
        }
    }

    graph
}

/// Recursively add module nodes and Contains edges
fn add_modules_recursive(
    graph: &mut ArcGraph,
    module: &ModuleInfo,
    crate_idx: NodeIndex,
    parent_idx: NodeIndex,
    module_map: &mut HashMap<String, NodeIndex>,
    module_deps: &mut Vec<(String, Vec<DependencyRef>)>,
) {
    // Add this module as a node
    let module_idx = graph.add_node(Node::Module {
        name: module.name.clone(),
        crate_idx,
    });
    module_map.insert(module.full_path.clone(), module_idx);

    // Store dependencies for Phase 4
    if !module.dependencies.is_empty() {
        module_deps.push((module.full_path.clone(), module.dependencies.clone()));
    }

    // Add Contains edge from parent to this module
    graph.add_edge(parent_idx, module_idx, Edge::Contains);

    // Recurse for children
    for child in &module.children {
        add_modules_recursive(graph, child, crate_idx, module_idx, module_map, module_deps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::{CrateInfo, DependencyRef, ModuleTree};
    use std::path::PathBuf;

    #[test]
    fn test_source_location_struct() {
        let loc = SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 42,
        };
        assert_eq!(loc.file, PathBuf::from("src/cli.rs"));
        assert_eq!(loc.line, 42);
    }

    #[test]
    fn test_moduledep_edge_carries_locations() {
        let edge = Edge::ModuleDep(vec![
            SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 5,
            },
            SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 12,
            },
        ]);
        if let Edge::ModuleDep(locs) = edge {
            assert_eq!(locs.len(), 2);
            assert_eq!(locs[0].line, 5);
            assert_eq!(locs[1].line, 12);
        } else {
            panic!("Expected ModuleDep");
        }
    }

    #[test]
    fn test_node_creation() {
        let crate_node = Node::Crate {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
        };
        let module_node = Node::Module {
            name: "my_module".to_string(),
            crate_idx: NodeIndex::new(0),
        };
        // Nodes should be creatable
        match crate_node {
            Node::Crate { name, .. } => assert_eq!(name, "my_crate"),
            _ => panic!("Expected Crate node"),
        }
        match module_node {
            Node::Module { name, .. } => assert_eq!(name, "my_module"),
            _ => panic!("Expected Module node"),
        }
    }

    #[test]
    fn test_edge_types() {
        let edges = [Edge::CrateDep, Edge::ModuleDep(vec![]), Edge::Contains];
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn test_build_graph_single_crate() {
        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
        }];
        let modules: Vec<ModuleTree> = vec![];

        let graph = build_graph(&crates, &modules);

        // Should have exactly one Crate node
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);

        // Verify the node is a Crate with correct name
        let node_idx = graph.node_indices().next().unwrap();
        match &graph[node_idx] {
            Node::Crate { name, .. } => assert_eq!(name, "my_crate"),
            _ => panic!("Expected Crate node"),
        }
    }

    #[test]
    fn test_build_graph_with_modules() {
        use crate::analyze::ModuleInfo;

        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
        }];

        let modules = vec![ModuleTree {
            root: ModuleInfo {
                name: "my_crate".to_string(),
                full_path: "crate".to_string(),
                children: vec![
                    ModuleInfo {
                        name: "foo".to_string(),
                        full_path: "crate::foo".to_string(),
                        children: vec![],
                        dependencies: vec![],
                    },
                    ModuleInfo {
                        name: "bar".to_string(),
                        full_path: "crate::bar".to_string(),
                        children: vec![],
                        dependencies: vec![],
                    },
                ],
                dependencies: vec![],
            },
        }];

        let graph = build_graph(&crates, &modules);

        // 1 Crate + 2 Modules = 3 nodes
        assert_eq!(graph.node_count(), 3, "expected 3 nodes");

        // 2 Contains edges (crate -> foo, crate -> bar)
        assert_eq!(graph.edge_count(), 2, "expected 2 Contains edges");

        // Verify edges are Contains type
        for edge_idx in graph.edge_indices() {
            match graph[edge_idx] {
                Edge::Contains => {}
                _ => panic!("Expected Contains edge"),
            }
        }
    }

    #[test]
    fn test_build_graph_crate_deps() {
        let crates = vec![
            CrateInfo {
                name: "crate_a".to_string(),
                path: PathBuf::from("/path/to/a"),
                dependencies: vec!["crate_b".to_string()],
            },
            CrateInfo {
                name: "crate_b".to_string(),
                path: PathBuf::from("/path/to/b"),
                dependencies: vec![],
            },
        ];
        let modules: Vec<ModuleTree> = vec![];

        let graph = build_graph(&crates, &modules);

        // 2 Crate nodes
        assert_eq!(graph.node_count(), 2);

        // 1 CrateDep edge (a -> b)
        assert_eq!(graph.edge_count(), 1);

        // Verify the edge is CrateDep
        let edge_idx = graph.edge_indices().next().unwrap();
        match graph[edge_idx] {
            Edge::CrateDep => {}
            _ => panic!("Expected CrateDep edge"),
        }
    }

    #[test]
    fn test_build_graph_module_deps() {
        use crate::analyze::ModuleInfo;

        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
        }];

        // Module "bar" depends on module "foo"
        let modules = vec![ModuleTree {
            root: ModuleInfo {
                name: "my_crate".to_string(),
                full_path: "crate".to_string(),
                children: vec![
                    ModuleInfo {
                        name: "foo".to_string(),
                        full_path: "crate::foo".to_string(),
                        children: vec![],
                        dependencies: vec![],
                    },
                    ModuleInfo {
                        name: "bar".to_string(),
                        full_path: "crate::bar".to_string(),
                        children: vec![],
                        dependencies: vec![DependencyRef {
                            target_crate: "crate".to_string(),
                            target_module: "foo".to_string(),
                            target_item: None,
                            source_file: PathBuf::from("src/bar.rs"),
                            line: 1,
                        }],
                    },
                ],
                dependencies: vec![],
            },
        }];

        let graph = build_graph(&crates, &modules);

        // 1 Crate + 2 Modules = 3 nodes
        assert_eq!(graph.node_count(), 3);

        // 2 Contains edges + 1 ModuleDep edge = 3 edges
        assert_eq!(graph.edge_count(), 3);

        // Count edge types
        let mut contains_count = 0;
        let mut module_dep_count = 0;
        for edge_idx in graph.edge_indices() {
            match graph[edge_idx] {
                Edge::Contains => contains_count += 1,
                Edge::ModuleDep(_) => module_dep_count += 1,
                _ => {}
            }
        }
        assert_eq!(contains_count, 2, "expected 2 Contains edges");
        assert_eq!(module_dep_count, 1, "expected 1 ModuleDep edge");
    }
}
