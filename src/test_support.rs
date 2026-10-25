//! Test-only constructors for model and graph types.

use crate::analyze::packages::normalize_crate_name;
use crate::graph::Node;
use crate::model::{EntryFile, LanguageProfile, PackageInfo, Wording};
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A made-up language whose packages are widgets, declared in `widgets.cfg`.
pub(crate) static WIDGETS: LanguageProfile = LanguageProfile {
    wording: Wording {
        package: "widget",
        packages: "widgets",
        manifest: "widgets.cfg",
    },
    extensions: &["lua"],
    manifest_files: &["widgets.cfg"],
    tokei_language: tokei::LanguageType::Lua,
    analyzes_tests: false,
    analyzes_externals: false,
};

/// A package node at `/<name>` with no entry files, for tests that only need
/// the graph shape.
pub(crate) fn package_node(name: &str) -> Node {
    Node::Package {
        name: name.to_string(),
        path: PathBuf::from(format!("/{name}")),
        entry_files: Vec::new(),
        manifest: Some(PathBuf::from(format!("/{name}/Cargo.toml"))),
    }
}

/// A module node without a file, for tests that only need the graph shape.
pub(crate) fn module_node(name: &str, package_idx: NodeIndex) -> Node {
    Node::Module {
        name: name.to_string(),
        package_idx,
        file: None,
        definitions: HashMap::new(),
    }
}

/// A package node at `/<name>` with the given entry files and manifest.
pub(crate) fn package_node_with_targets(
    name: &str,
    entry_files: Vec<EntryFile>,
    manifest: Option<&str>,
) -> Node {
    Node::Package {
        name: name.to_string(),
        path: PathBuf::from(format!("/{name}")),
        entry_files,
        manifest: manifest.map(PathBuf::from),
    }
}

/// A module node with the given declaring file.
pub(crate) fn module_node_with_file(
    name: &str,
    package_idx: NodeIndex,
    file: impl Into<PathBuf>,
) -> Node {
    Node::Module {
        name: name.to_string(),
        package_idx,
        file: Some(file.into()),
        definitions: HashMap::new(),
    }
}

/// Run `git` in `dir` and return its trimmed stdout, failing the test on a
/// non-zero exit. It is cut off from any repository the environment points
/// at, so a test run under `git rebase --exec` never writes into that one.
pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        output.status.success(),
        "git {args:?} failed in {}",
        dir.display()
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A package laid out by Cargo's default convention, with its roots probed from
/// disk. Hand-built fixtures carry no manifest, so there is no cargo metadata
/// to resolve targets from.
pub(crate) fn conventional_package(name: &str, path: impl Into<PathBuf>) -> PackageInfo {
    let path = path.into();
    let existing = |p: PathBuf| p.exists().then_some(p);
    PackageInfo {
        name: name.to_string(),
        key: normalize_crate_name(name),
        code_names: HashMap::new(),
        entry_files: existing(path.join("src/lib.rs"))
            .map(EntryFile::lib)
            .into_iter()
            .chain(existing(path.join("src/main.rs")).map(EntryFile::bin))
            .collect(),
        manifest: Some(path.join("Cargo.toml")),
        workspace_root: path.clone(),
        path,
        dependencies: Vec::new(),
        dev_dependencies: Vec::new(),
    }
}
