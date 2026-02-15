//! Graph Types & Builder

use crate::model::{
    CrateInfo, DependencyKind, DependencyRef, EdgeContext, ModuleInfo, ModuleTree, SourceLocation,
    TestKind,
};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Node {
    Crate { name: String, path: PathBuf },
    Module { name: String, crate_idx: NodeIndex },
}

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

            // Root-Dependencies (lib.rs/main.rs) aufnehmen
            if !module_tree.root.dependencies.is_empty() {
                module_deps.push((
                    module_tree.root.name.clone(),
                    module_tree.root.dependencies.clone(),
                ));
            }
        }
    }

    // Phase 3: Add CrateDep edges
    for crate_info in crates {
        if let Some(&from_idx) = crate_map.get(&crate_info.name) {
            for dep_name in &crate_info.dependencies {
                if let Some(&to_idx) = crate_map.get(dep_name) {
                    graph.add_edge(
                        from_idx,
                        to_idx,
                        Edge::CrateDep {
                            context: EdgeContext::production(),
                        },
                    );
                }
            }
            for dep_name in &crate_info.dev_dependencies {
                if let Some(&to_idx) = crate_map.get(dep_name) {
                    graph.add_edge(
                        from_idx,
                        to_idx,
                        Edge::CrateDep {
                            context: EdgeContext::test(TestKind::Unit),
                        },
                    );
                }
            }
        }
    }

    // Phase 4: Add ModuleDep edges (aggregate symbols per module target)
    for (from_path, deps) in &module_deps {
        if let Some(&from_idx) = module_map
            .get(from_path)
            .or_else(|| crate_map.get(from_path))
            .or_else(|| crate_map.get(&from_path.replace('_', "-")))
        {
            // Group deps by module_target to aggregate symbols into one edge.
            // Context is derived from the group: Production if any dep is Production,
            // otherwise Test. This ensures at most one edge per (from, to) node pair,
            // which the rendering pipeline requires (edge_id = "from-to").
            let mut grouped: HashMap<String, Vec<&DependencyRef>> = HashMap::new();
            for dep in deps {
                grouped.entry(dep.module_target()).or_default().push(dep);
            }

            let mut sorted_targets: Vec<_> = grouped.into_iter().collect();
            sorted_targets.sort_by(|(a, _), (b, _)| a.cmp(b));

            for (target, target_deps) in sorted_targets {
                // Derive context: Production wins over Test.
                // Note: creates a fresh context with empty features. When we
                // populate features in context of feature exclusion, we need to
                // collect them from all production dependencies here.
                let context = if target_deps
                    .iter()
                    .any(|d| d.context.kind == DependencyKind::Production)
                {
                    EdgeContext::production()
                } else {
                    target_deps[0].context.clone()
                };
                if let Some(&to_idx) = module_map
                    .get(&target)
                    .or_else(|| crate_map.get(&target))
                    .or_else(|| crate_map.get(&target.replace('_', "-")))
                {
                    let module_path = if target_deps[0].target_module.is_empty() {
                        target.clone()
                    } else {
                        target_deps[0].target_module.clone()
                    };
                    // Collect all symbols from deps at the same location, or create one SourceLocation per line
                    let mut locations_by_line: HashMap<(PathBuf, usize), Vec<String>> =
                        HashMap::new();
                    for dep in &target_deps {
                        let key = (dep.source_file.clone(), dep.line);
                        if let Some(item) = &dep.target_item {
                            locations_by_line.entry(key).or_default().push(item.clone());
                        } else {
                            locations_by_line.entry(key).or_default();
                        }
                    }

                    let mut sorted_locations: Vec<_> = locations_by_line.into_iter().collect();
                    sorted_locations.sort_by(|(a, _), (b, _)| a.cmp(b));

                    let locations: Vec<SourceLocation> = sorted_locations
                        .into_iter()
                        .map(|((file, line), symbols)| SourceLocation {
                            file,
                            line,
                            symbols,
                            module_path: module_path.clone(),
                        })
                        .collect();

                    // Skip self-loops (e.g. main.rs importing from own crate's re-exports)
                    if from_idx != to_idx {
                        graph.add_edge(from_idx, to_idx, Edge::ModuleDep { locations, context });
                    }
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
    use crate::model::{CrateInfo, DependencyRef, ModuleTree};
    use std::path::PathBuf;

    #[test]
    fn test_source_location_struct() {
        let loc = SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 42,
            symbols: vec![],
            module_path: String::new(),
        };
        assert_eq!(loc.file, PathBuf::from("src/cli.rs"));
        assert_eq!(loc.line, 42);
    }

    #[test]
    fn test_source_location_with_symbols() {
        let loc = SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 42,
            symbols: vec!["ModuleInfo".to_string()],
            module_path: String::new(),
        };
        assert_eq!(loc.symbols.len(), 1);
        assert_eq!(loc.symbols[0], "ModuleInfo");
    }

    #[test]
    fn test_moduledep_edge_carries_locations() {
        let edge = Edge::ModuleDep {
            locations: vec![
                SourceLocation {
                    file: PathBuf::from("src/cli.rs"),
                    line: 5,
                    symbols: vec![],
                    module_path: String::new(),
                },
                SourceLocation {
                    file: PathBuf::from("src/cli.rs"),
                    line: 12,
                    symbols: vec![],
                    module_path: String::new(),
                },
            ],
            context: EdgeContext::production(),
        };
        if let Edge::ModuleDep {
            locations: locs, ..
        } = edge
        {
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
        let edges = [
            Edge::CrateDep {
                context: EdgeContext::production(),
            },
            Edge::ModuleDep {
                locations: vec![],
                context: EdgeContext::production(),
            },
            Edge::Contains,
        ];
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn test_build_graph_single_crate() {
        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
            dev_dependencies: vec![],
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
        use crate::model::ModuleInfo;

        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
            dev_dependencies: vec![],
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
                dev_dependencies: vec![],
            },
            CrateInfo {
                name: "crate_b".to_string(),
                path: PathBuf::from("/path/to/b"),
                dependencies: vec![],
                dev_dependencies: vec![],
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
            Edge::CrateDep { .. } => {}
            _ => panic!("Expected CrateDep edge"),
        }
    }

    #[test]
    fn test_build_graph_module_deps() {
        use crate::model::ModuleInfo;

        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
            dev_dependencies: vec![],
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
                            context: EdgeContext::production(),
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
                Edge::ModuleDep { .. } => module_dep_count += 1,
                _ => {}
            }
        }
        assert_eq!(contains_count, 2, "expected 2 Contains edges");
        assert_eq!(module_dep_count, 1, "expected 1 ModuleDep edge");
    }

    #[test]
    fn test_build_graph_inter_crate_module_deps() {
        use crate::model::ModuleInfo;

        // Two crates: crate_a depends on crate_b
        let crates = vec![
            CrateInfo {
                name: "crate_a".to_string(),
                path: PathBuf::from("/path/to/a"),
                dependencies: vec!["crate_b".to_string()],
                dev_dependencies: vec![],
            },
            CrateInfo {
                name: "crate_b".to_string(),
                path: PathBuf::from("/path/to/b"),
                dependencies: vec![],
                dev_dependencies: vec![],
            },
        ];

        // crate_a::beta depends on crate_b::gamma (inter-crate module dep)
        let modules = vec![
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_a".to_string(),
                    full_path: "crate_a".to_string(),
                    children: vec![ModuleInfo {
                        name: "beta".to_string(),
                        full_path: "crate_a::beta".to_string(),
                        children: vec![],
                        dependencies: vec![DependencyRef {
                            target_crate: "crate_b".to_string(),
                            target_module: "gamma".to_string(),
                            target_item: None,
                            source_file: PathBuf::from("src/beta.rs"),
                            line: 1,
                            context: EdgeContext::production(),
                        }],
                    }],
                    dependencies: vec![],
                },
            },
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_b".to_string(),
                    full_path: "crate_b".to_string(),
                    children: vec![ModuleInfo {
                        name: "gamma".to_string(),
                        full_path: "crate_b::gamma".to_string(),
                        children: vec![],
                        dependencies: vec![],
                    }],
                    dependencies: vec![],
                },
            },
        ];

        let graph = build_graph(&crates, &modules);

        // 2 Crates + 2 Modules = 4 nodes
        assert_eq!(graph.node_count(), 4);

        // 1 CrateDep + 2 Contains + 1 ModuleDep = 4 edges
        assert_eq!(graph.edge_count(), 4);

        // Count edge types
        let mut crate_dep_count = 0;
        let mut contains_count = 0;
        let mut module_dep_count = 0;
        for edge_idx in graph.edge_indices() {
            match &graph[edge_idx] {
                Edge::CrateDep { .. } => crate_dep_count += 1,
                Edge::Contains => contains_count += 1,
                Edge::ModuleDep {
                    locations: locs, ..
                } => {
                    module_dep_count += 1;
                    // Verify source location is preserved
                    assert_eq!(locs.len(), 1);
                    assert_eq!(locs[0].file, PathBuf::from("src/beta.rs"));
                    assert_eq!(locs[0].line, 1);
                }
            }
        }
        assert_eq!(crate_dep_count, 1, "expected 1 CrateDep edge");
        assert_eq!(contains_count, 2, "expected 2 Contains edges");
        assert_eq!(module_dep_count, 1, "expected 1 inter-crate ModuleDep edge");
    }

    /// Test A: Root dependencies (from lib.rs/main.rs) produce ModuleDep edges
    /// from the Crate node to a Module node.
    #[test]
    fn test_root_dependencies_in_module_deps() {
        use crate::model::ModuleInfo;

        let crates = vec![CrateInfo {
            name: "crate_a".to_string(),
            path: PathBuf::from("/path/to/a"),
            dependencies: vec![],
            dev_dependencies: vec![],
        }];

        // Root module has a dependency on child module "gamma"
        let modules = vec![ModuleTree {
            root: ModuleInfo {
                name: "crate_a".to_string(),
                full_path: "crate_a".to_string(),
                children: vec![ModuleInfo {
                    name: "gamma".to_string(),
                    full_path: "crate_a::gamma".to_string(),
                    children: vec![],
                    dependencies: vec![],
                }],
                dependencies: vec![DependencyRef {
                    target_crate: "crate_a".to_string(),
                    target_module: "gamma".to_string(),
                    target_item: None,
                    source_file: PathBuf::from("src/lib.rs"),
                    line: 5,
                    context: EdgeContext::production(),
                }],
            },
        }];

        let graph = build_graph(&crates, &modules);

        // Should have ModuleDep edge from Crate node to gamma Module node
        let mut module_dep_count = 0;
        for edge_idx in graph.edge_indices() {
            if let Edge::ModuleDep {
                locations: locs, ..
            } = &graph[edge_idx]
            {
                module_dep_count += 1;
                let (from, to) = graph.edge_endpoints(edge_idx).unwrap();
                assert!(
                    matches!(&graph[from], Node::Crate { .. }),
                    "from should be Crate node"
                );
                assert!(
                    matches!(&graph[to], Node::Module { name, .. } if name == "gamma"),
                    "to should be gamma Module node"
                );
                assert_eq!(locs[0].file, PathBuf::from("src/lib.rs"));
            }
        }
        assert_eq!(module_dep_count, 1, "expected 1 ModuleDep from root");
    }

    /// Test B: Entry-point dependency (target_module="") from a Module to a Crate node.
    /// module_path should be the crate name, not empty.
    #[test]
    fn test_module_dep_to_crate_node() {
        use crate::model::ModuleInfo;

        let crates = vec![
            CrateInfo {
                name: "crate_a".to_string(),
                path: PathBuf::from("/path/to/a"),
                dependencies: vec!["crate_b".to_string()],
                dev_dependencies: vec![],
            },
            CrateInfo {
                name: "crate_b".to_string(),
                path: PathBuf::from("/path/to/b"),
                dependencies: vec![],
                dev_dependencies: vec![],
            },
        ];

        // crate_a::beta depends on crate_b entry-point (target_module="")
        let modules = vec![
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_a".to_string(),
                    full_path: "crate_a".to_string(),
                    children: vec![ModuleInfo {
                        name: "beta".to_string(),
                        full_path: "crate_a::beta".to_string(),
                        children: vec![],
                        dependencies: vec![DependencyRef {
                            target_crate: "crate_b".to_string(),
                            target_module: "".to_string(),
                            target_item: Some("Widget".to_string()),
                            source_file: PathBuf::from("src/beta.rs"),
                            line: 3,
                            context: EdgeContext::production(),
                        }],
                    }],
                    dependencies: vec![],
                },
            },
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_b".to_string(),
                    full_path: "crate_b".to_string(),
                    children: vec![],
                    dependencies: vec![],
                },
            },
        ];

        let graph = build_graph(&crates, &modules);

        // Should have ModuleDep edge from beta Module to crate_b Crate node
        let mut found = false;
        for edge_idx in graph.edge_indices() {
            if let Edge::ModuleDep {
                locations: locs, ..
            } = &graph[edge_idx]
            {
                let (from, to) = graph.edge_endpoints(edge_idx).unwrap();
                if matches!(&graph[from], Node::Module { name, .. } if name == "beta")
                    && matches!(&graph[to], Node::Crate { name, .. } if name == "crate_b")
                {
                    found = true;
                    assert_eq!(
                        locs[0].module_path, "crate_b",
                        "module_path should be crate name, not empty"
                    );
                    assert_eq!(locs[0].symbols, vec!["Widget"]);
                }
            }
        }
        assert!(found, "expected ModuleDep from beta to crate_b Crate node");
    }

    /// Test C: Root dependency from Crate A to a Module in Crate B.
    #[test]
    fn test_root_dep_to_module() {
        use crate::model::ModuleInfo;

        let crates = vec![
            CrateInfo {
                name: "crate_a".to_string(),
                path: PathBuf::from("/path/to/a"),
                dependencies: vec!["crate_b".to_string()],
                dev_dependencies: vec![],
            },
            CrateInfo {
                name: "crate_b".to_string(),
                path: PathBuf::from("/path/to/b"),
                dependencies: vec![],
                dev_dependencies: vec![],
            },
        ];

        // crate_a root depends on crate_b::gamma
        let modules = vec![
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_a".to_string(),
                    full_path: "crate_a".to_string(),
                    children: vec![],
                    dependencies: vec![DependencyRef {
                        target_crate: "crate_b".to_string(),
                        target_module: "gamma".to_string(),
                        target_item: None,
                        source_file: PathBuf::from("src/lib.rs"),
                        line: 2,
                        context: EdgeContext::production(),
                    }],
                },
            },
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_b".to_string(),
                    full_path: "crate_b".to_string(),
                    children: vec![ModuleInfo {
                        name: "gamma".to_string(),
                        full_path: "crate_b::gamma".to_string(),
                        children: vec![],
                        dependencies: vec![],
                    }],
                    dependencies: vec![],
                },
            },
        ];

        let graph = build_graph(&crates, &modules);

        // Should have ModuleDep from crate_a Crate node to gamma Module node
        let mut found = false;
        for edge_idx in graph.edge_indices() {
            if let Edge::ModuleDep {
                locations: locs, ..
            } = &graph[edge_idx]
            {
                let (from, to) = graph.edge_endpoints(edge_idx).unwrap();
                if matches!(&graph[from], Node::Crate { name, .. } if name == "crate_a")
                    && matches!(&graph[to], Node::Module { name, .. } if name == "gamma")
                {
                    found = true;
                    assert_eq!(locs[0].file, PathBuf::from("src/lib.rs"));
                }
            }
        }
        assert!(found, "expected ModuleDep from crate_a to gamma");
    }

    /// Test D: Root dependency with entry-point (target_module="") → Crate-to-Crate ModuleDep.
    /// module_path should be the crate name, not empty.
    #[test]
    fn test_root_dep_to_crate_node() {
        use crate::model::ModuleInfo;

        let crates = vec![
            CrateInfo {
                name: "crate_a".to_string(),
                path: PathBuf::from("/path/to/a"),
                dependencies: vec!["crate_b".to_string()],
                dev_dependencies: vec![],
            },
            CrateInfo {
                name: "crate_b".to_string(),
                path: PathBuf::from("/path/to/b"),
                dependencies: vec![],
                dev_dependencies: vec![],
            },
        ];

        // crate_a root depends on crate_b entry-point (target_module="")
        let modules = vec![
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_a".to_string(),
                    full_path: "crate_a".to_string(),
                    children: vec![],
                    dependencies: vec![DependencyRef {
                        target_crate: "crate_b".to_string(),
                        target_module: "".to_string(),
                        target_item: Some("Config".to_string()),
                        source_file: PathBuf::from("src/lib.rs"),
                        line: 1,
                        context: EdgeContext::production(),
                    }],
                },
            },
            ModuleTree {
                root: ModuleInfo {
                    name: "crate_b".to_string(),
                    full_path: "crate_b".to_string(),
                    children: vec![],
                    dependencies: vec![],
                },
            },
        ];

        let graph = build_graph(&crates, &modules);

        // Should have ModuleDep from crate_a Crate to crate_b Crate
        let mut found = false;
        for edge_idx in graph.edge_indices() {
            if let Edge::ModuleDep {
                locations: locs, ..
            } = &graph[edge_idx]
            {
                let (from, to) = graph.edge_endpoints(edge_idx).unwrap();
                if matches!(&graph[from], Node::Crate { name, .. } if name == "crate_a")
                    && matches!(&graph[to], Node::Crate { name, .. } if name == "crate_b")
                {
                    found = true;
                    assert_eq!(
                        locs[0].module_path, "crate_b",
                        "module_path should be crate name"
                    );
                    assert_eq!(locs[0].symbols, vec!["Config"]);
                }
            }
        }
        assert!(
            found,
            "expected ModuleDep from crate_a Crate to crate_b Crate"
        );
    }

    #[test]
    fn test_cfg_test_dep_creates_test_edge() {
        use crate::model::ModuleInfo;

        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
            dev_dependencies: vec![],
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
                        dependencies: vec![DependencyRef {
                            target_crate: "crate".to_string(),
                            target_module: "foo".to_string(),
                            target_item: Some("helper".to_string()),
                            source_file: PathBuf::from("src/bar.rs"),
                            line: 5,
                            context: EdgeContext::test(TestKind::Unit),
                        }],
                    },
                ],
                dependencies: vec![],
            },
        }];

        let graph = build_graph(&crates, &modules);

        let mut found_test_edge = false;
        for edge_idx in graph.edge_indices() {
            if let Edge::ModuleDep { context, .. } = &graph[edge_idx] {
                if *context == EdgeContext::test(TestKind::Unit) {
                    found_test_edge = true;
                }
            }
        }
        assert!(
            found_test_edge,
            "Test(Unit) dep should create ModuleDep with Test(Unit) context"
        );
    }

    #[test]
    fn test_mixed_context_merges_into_production_edge() {
        use crate::model::ModuleInfo;

        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/path/to/crate"),
            dependencies: vec![],
            dev_dependencies: vec![],
        }];

        // bar depends on foo twice: once Production, once Test(Unit)
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
                        dependencies: vec![
                            DependencyRef {
                                target_crate: "crate".to_string(),
                                target_module: "foo".to_string(),
                                target_item: Some("run".to_string()),
                                source_file: PathBuf::from("src/bar.rs"),
                                line: 1,
                                context: EdgeContext::production(),
                            },
                            DependencyRef {
                                target_crate: "crate".to_string(),
                                target_module: "foo".to_string(),
                                target_item: Some("test_helper".to_string()),
                                source_file: PathBuf::from("src/bar.rs"),
                                line: 10,
                                context: EdgeContext::test(TestKind::Unit),
                            },
                        ],
                    },
                ],
                dependencies: vec![],
            },
        }];

        let graph = build_graph(&crates, &modules);

        // Mixed contexts merge into one edge with Production context,
        // preserving all locations (rendering requires one edge per node pair)
        let mut edge_count = 0;
        let mut context_is_production = false;
        let mut location_count = 0;
        for edge_idx in graph.edge_indices() {
            if let Edge::ModuleDep { context, locations } = &graph[edge_idx] {
                let (from, to) = graph.edge_endpoints(edge_idx).unwrap();
                if matches!(&graph[from], Node::Module { name, .. } if name == "bar")
                    && matches!(&graph[to], Node::Module { name, .. } if name == "foo")
                {
                    edge_count += 1;
                    context_is_production = *context == EdgeContext::production();
                    location_count = locations.len();
                }
            }
        }
        assert_eq!(edge_count, 1, "mixed contexts should merge into one edge");
        assert!(
            context_is_production,
            "merged edge should have Production context"
        );
        assert_eq!(location_count, 2, "both locations should be preserved");
    }
}
