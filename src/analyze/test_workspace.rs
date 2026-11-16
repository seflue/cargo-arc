//! A workspace written as Rust source to a temporary directory and analyzed
//! with the syn backend, so that name-resolution tests read dependencies the
//! way the analysis hands them out.

use std::collections::HashMap;
use std::path::Path;

use tempfile::TempDir;

use super::name_resolution::NameResolution;
use super::packages::ModulePathMap;
use super::syn_walker::{analyze_modules_syn, collect_syn_module_paths};
use petgraph::visit::EdgeRef;

use crate::graph::{self, ArcGraph, EdgeWeight, Usage};
use crate::model::{DependencyRef, ModuleInfo, ModuleTree, PackageInfo};
use crate::test_support::conventional_package;

/// The packages of a workspace, each as files relative to its own directory.
#[derive(Default)]
pub(crate) struct TestWorkspace {
    packages: Vec<(String, Vec<(String, String)>)>,
    external_crates: HashMap<String, HashMap<String, String>>,
}

impl TestWorkspace {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Add the package `name` with its `files`, such as `("src/lib.rs", "mod a;")`.
    pub(crate) fn package(mut self, name: &str, files: &[(&str, &str)]) -> Self {
        let files = files
            .iter()
            .map(|(path, source)| ((*path).to_string(), (*source).to_string()))
            .collect();
        self.packages.push((name.to_string(), files));
        self
    }

    /// Let the package `name` reach the external crate `code_name`, which the
    /// externals analysis reports with the package id `package_id`.
    pub(crate) fn external(mut self, name: &str, code_name: &str, package_id: &str) -> Self {
        self.external_crates
            .entry(name.to_string())
            .or_default()
            .insert(code_name.to_string(), package_id.to_string());
        self
    }

    /// Analyze the workspace without its test code.
    pub(crate) fn analyze(self) -> Analyzed {
        self.analyze_with(false)
    }

    /// Analyze the workspace together with its test code, as `--include-tests` does.
    pub(crate) fn analyze_with_tests(self) -> Analyzed {
        self.analyze_with(true)
    }

    fn analyze_with(self, include_tests: bool) -> Analyzed {
        let dir = TempDir::new().expect("a temporary directory");
        let mut packages: Vec<PackageInfo> = self
            .packages
            .iter()
            .map(|(name, files)| write_package(dir.path(), name, files))
            .collect();
        // Each package depends on every other one, so each names the others.
        let keys: Vec<String> = packages.iter().map(|p| p.key.clone()).collect();
        for package in &mut packages {
            package.dependencies = keys
                .iter()
                .filter(|key| **key != package.key)
                .cloned()
                .collect();
        }
        let module_paths: ModulePathMap = packages
            .iter()
            .map(|p| (p.key.clone(), collect_syn_module_paths(p, include_tests)))
            .collect();
        let resolution = NameResolution::new(&packages, &module_paths, include_tests);
        let no_externals = HashMap::new();
        let trees = packages
            .iter()
            .map(|p| {
                let externals = self.external_crates.get(&p.name).unwrap_or(&no_externals);
                let tree = analyze_modules_syn(p, &resolution, externals);
                (p.key.clone(), tree)
            })
            .collect();
        Analyzed {
            packages,
            trees,
            _dir: dir,
        }
    }
}

fn write_package(root: &Path, name: &str, files: &[(String, String)]) -> PackageInfo {
    let dir = root.join(name);
    for (path, source) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a file sits in a directory"))
            .expect("the package directory is writable");
        std::fs::write(&file, source).expect("the source file is writable");
    }
    PackageInfo {
        workspace_root: root.to_path_buf(),
        ..conventional_package(name, dir)
    }
}

/// The module trees of an analyzed workspace, by package key.
pub(crate) struct Analyzed {
    packages: Vec<PackageInfo>,
    trees: HashMap<String, ModuleTree>,
    _dir: TempDir,
}

impl Analyzed {
    /// The module or binary at `path`, its `full_path`.
    pub(crate) fn module(&self, path: &str) -> &ModuleInfo {
        let package = path.split("::").next().unwrap_or(path);
        let tree = self
            .trees
            .get(package)
            .unwrap_or_else(|| panic!("no package {package}"));
        std::iter::once(&tree.root)
            .chain(&tree.binaries)
            .find_map(|root| find_module(root, path))
            .unwrap_or_else(|| panic!("no module {path}"))
    }

    /// The usages `query usages` lists for the workspace, in the order
    /// [`graph::usages`] returns them.
    pub(crate) fn usages(&self) -> Vec<Usage> {
        let trees: Vec<ModuleTree> = self
            .packages
            .iter()
            .filter_map(|package| self.trees.get(&package.key).cloned())
            .collect();
        graph::usages(&self.packages, &trees)
    }

    /// The module edges of the diagram's graph, each as the qualified names
    /// of its ends, sorted.
    pub(crate) fn module_edges(&self) -> Vec<(String, String)> {
        let trees: Vec<ModuleTree> = self
            .packages
            .iter()
            .filter_map(|package| self.trees.get(&package.key).cloned())
            .collect();
        let graph = ArcGraph::build(&self.packages, &trees, None, false);
        let mut edges: Vec<(String, String)> = graph
            .edge_references()
            .filter(|edge| matches!(edge.weight(), EdgeWeight::ModuleDep { .. }))
            .map(|edge| {
                (
                    graph.qualified_name(edge.source()),
                    graph.qualified_name(edge.target()),
                )
            })
            .collect();
        edges.sort();
        edges
    }

    /// The dependencies of the module at `path`.
    pub(crate) fn deps(&self, path: &str) -> &[DependencyRef] {
        &self.module(path).dependencies
    }

    /// The targets of the module at `path` as package, module and item, in the
    /// order the analysis lists them.
    pub(crate) fn targets(&self, path: &str) -> Vec<(&str, &str, Option<&str>)> {
        self.deps(path)
            .iter()
            .map(|dep| {
                (
                    dep.target_package.as_str(),
                    dep.target_module.as_str(),
                    dep.target_item.as_deref(),
                )
            })
            .collect()
    }

    /// The targets of the module at `path`, sorted.
    pub(crate) fn sorted_targets(&self, path: &str) -> Vec<(&str, &str, Option<&str>)> {
        let mut targets = self.targets(path);
        targets.sort_unstable();
        targets
    }

    /// The dependency of the module at `path` on `item`.
    pub(crate) fn dep_on(&self, path: &str, item: &str) -> &DependencyRef {
        self.deps(path)
            .iter()
            .find(|dep| dep.target_item.as_deref() == Some(item))
            .unwrap_or_else(|| panic!("{path} has no dependency on {item}: {:?}", self.deps(path)))
    }
}

fn find_module<'a>(module: &'a ModuleInfo, path: &str) -> Option<&'a ModuleInfo> {
    if module.full_path == path {
        return Some(module);
    }
    module
        .children
        .iter()
        .find_map(|child| find_module(child, path))
}
