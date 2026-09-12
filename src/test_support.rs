//! Test-only constructors for model and graph types.

use crate::graph::Node;
use crate::model::{CrateInfo, TargetRoots};
use petgraph::graph::NodeIndex;
use std::path::PathBuf;

/// A crate node at `/<name>` with no target roots, for tests that only need
/// the graph shape.
pub(crate) fn crate_node(name: &str) -> Node {
    Node::Crate {
        name: name.to_string(),
        path: PathBuf::from(format!("/{name}")),
        target_roots: TargetRoots::default(),
        manifest: PathBuf::from(format!("/{name}/Cargo.toml")),
    }
}

/// A module node without a file, for tests that only need the graph shape.
pub(crate) fn module_node(name: &str, crate_idx: NodeIndex) -> Node {
    Node::Module {
        name: name.to_string(),
        crate_idx,
        file: None,
    }
}

/// A crate laid out by Cargo's default convention, with its roots probed from
/// disk. Hand-built fixtures carry no manifest, so there is no cargo metadata
/// to resolve targets from.
pub(crate) fn conventional_crate(name: &str, path: impl Into<PathBuf>) -> CrateInfo {
    let path = path.into();
    let existing = |p: PathBuf| p.exists().then_some(p);
    CrateInfo {
        name: name.to_string(),
        target_roots: TargetRoots {
            lib_root: existing(path.join("src/lib.rs")),
            bin_roots: existing(path.join("src/main.rs")).into_iter().collect(),
        },
        manifest: path.join("Cargo.toml"),
        workspace_root: path.clone(),
        path,
        dependencies: Vec::new(),
        dev_dependencies: Vec::new(),
    }
}
