//! Shared Data Structures
//!
//! Types used across analyze and graph modules, extracted to break circular dependencies.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::PathBuf;

/// Workspace crate names (e.g. `{"cargo-arc", "my_lib"}`).
#[derive(Debug, Default, Clone)]
pub struct WorkspaceCrates(HashSet<String>);

impl Deref for WorkspaceCrates {
    type Target = HashSet<String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<String> for WorkspaceCrates {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl WorkspaceCrates {
    pub fn insert(&mut self, name: String) -> bool {
        self.0.insert(name)
    }
}

/// Crate name → set of module paths (e.g. `{"analyze", "analyze::hir"}`).
#[derive(Debug, Default, Clone)]
pub struct ModulePathMap(HashMap<String, HashSet<String>>);

impl Deref for ModulePathMap {
    type Target = HashMap<String, HashSet<String>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<(String, HashSet<String>)> for ModulePathMap {
    fn from_iter<I: IntoIterator<Item = (String, HashSet<String>)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// Crate name → set of exported symbol names.
#[derive(Debug, Default, Clone)]
pub struct CrateExportMap(HashMap<String, HashSet<String>>);

impl Deref for CrateExportMap {
    type Target = HashMap<String, HashSet<String>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<(String, HashSet<String>)> for CrateExportMap {
    fn from_iter<I: IntoIterator<Item = (String, HashSet<String>)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeContext {
    Production,
    Test(TestKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestKind {
    Unit,
    Integration,
}

#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    pub path: PathBuf,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DependencyRef {
    pub target_crate: String,
    pub target_module: String,
    pub target_item: Option<String>,
    pub source_file: PathBuf,
    pub line: usize,
    pub context: EdgeContext,
}

impl DependencyRef {
    /// Returns full target path: "crate::module::item" or "crate::module" if no item.
    /// For empty target_module (entry-point): "crate::item" or just "crate".
    pub fn full_target(&self) -> String {
        match (&self.target_item, self.target_module.is_empty()) {
            (Some(item), true) => format!("{}::{}", self.target_crate, item),
            (Some(item), false) => {
                format!("{}::{}::{}", self.target_crate, self.target_module, item)
            }
            (None, true) => self.target_crate.clone(),
            (None, false) => format!("{}::{}", self.target_crate, self.target_module),
        }
    }

    /// Returns module-level target: "crate::module" (ignores item).
    /// For empty target_module (entry-point): just "crate".
    pub fn module_target(&self) -> String {
        if self.target_module.is_empty() {
            self.target_crate.clone()
        } else {
            format!("{}::{}", self.target_crate, self.target_module)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub full_path: String,
    pub children: Vec<ModuleInfo>,
    pub dependencies: Vec<DependencyRef>,
}

#[derive(Debug, Clone)]
pub struct ModuleTree {
    pub root: ModuleInfo,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_dependency_ref_carries_context() {
        let prod_dep = DependencyRef {
            target_crate: "my_crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::from("src/lib.rs"),
            line: 1,
            context: EdgeContext::Production,
        };
        assert_eq!(prod_dep.context, EdgeContext::Production);

        let test_dep = DependencyRef {
            target_crate: "my_crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::from("src/lib.rs"),
            line: 1,
            context: EdgeContext::Test(TestKind::Unit),
        };
        assert_eq!(test_dep.context, EdgeContext::Test(TestKind::Unit));

        // Different context → not equal (PartialEq includes context)
        assert_ne!(prod_dep, test_dep);
    }

    #[test]
    fn test_dependency_ref_struct() {
        let dep = DependencyRef {
            target_crate: "my_crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::from("src/cli.rs"),
            line: 42,
            context: EdgeContext::Production,
        };
        assert_eq!(dep.target_crate, "my_crate");
        assert_eq!(dep.target_module, "graph");
        assert!(dep.target_item.is_none());
        assert_eq!(dep.source_file, PathBuf::from("src/cli.rs"));
        assert_eq!(dep.line, 42);
    }

    #[test]
    fn test_dependency_ref_full_target() {
        let dep = DependencyRef {
            target_crate: "crate".to_string(),
            target_module: "graph".to_string(),
            target_item: Some("build".to_string()),
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::Production,
        };
        assert_eq!(dep.full_target(), "crate::graph::build");
    }

    #[test]
    fn test_dependency_ref_module_target() {
        let dep = DependencyRef {
            target_crate: "crate".to_string(),
            target_module: "graph".to_string(),
            target_item: Some("build".to_string()),
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::Production,
        };
        assert_eq!(dep.module_target(), "crate::graph");
    }

    #[test]
    fn test_dependency_ref_full_target_no_item() {
        let dep = DependencyRef {
            target_crate: "crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::Production,
        };
        assert_eq!(dep.full_target(), "crate::graph");
    }

    #[test]
    fn test_module_target_empty_module() {
        let dep = DependencyRef {
            target_crate: "crate_b".to_string(),
            target_module: "".to_string(),
            target_item: None,
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::Production,
        };
        assert_eq!(dep.module_target(), "crate_b");
    }

    #[test]
    fn test_full_target_empty_module_with_item() {
        let dep = DependencyRef {
            target_crate: "crate_b".to_string(),
            target_module: "".to_string(),
            target_item: Some("Symbol".to_string()),
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::Production,
        };
        assert_eq!(dep.full_target(), "crate_b::Symbol");
    }

    #[test]
    fn test_full_target_empty_module_no_item() {
        let dep = DependencyRef {
            target_crate: "crate_b".to_string(),
            target_module: "".to_string(),
            target_item: None,
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::Production,
        };
        assert_eq!(dep.full_target(), "crate_b");
    }

    #[test]
    fn test_module_info_has_dependency_refs() {
        let module = ModuleInfo {
            name: "cli".to_string(),
            full_path: "crate::cli".to_string(),
            children: vec![],
            dependencies: vec![DependencyRef {
                target_crate: "crate".to_string(),
                target_module: "graph".to_string(),
                target_item: None,
                source_file: PathBuf::from("src/cli.rs"),
                line: 5,
                context: EdgeContext::Production,
            }],
        };
        assert!(
            module
                .dependencies
                .iter()
                .any(|d| d.module_target() == "crate::graph")
        );
    }
}
