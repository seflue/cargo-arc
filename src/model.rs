//! Shared Data Structures
//!
//! Types used across analyze and graph modules, extracted to break circular dependencies.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::PathBuf;

/// Workspace crate names, stored in normalized form (hyphens → underscores).
///
/// All insertion paths normalize names, and `contains()` normalizes its input,
/// so lookups are O(1) and callers never need to think about normalization.
#[derive(Debug, Default, Clone)]
pub struct WorkspaceCrates(HashSet<String>);

impl FromIterator<String> for WorkspaceCrates {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self(iter.into_iter().map(|s| normalize_crate_name(&s)).collect())
    }
}

impl<'a> FromIterator<&'a str> for WorkspaceCrates {
    fn from_iter<I: IntoIterator<Item = &'a str>>(iter: I) -> Self {
        Self(iter.into_iter().map(normalize_crate_name).collect())
    }
}

impl WorkspaceCrates {
    pub fn insert(&mut self, name: String) -> bool {
        self.0.insert(normalize_crate_name(&name))
    }

    /// Check membership, normalizing the input name.
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(&normalize_crate_name(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub(crate) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
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
    fn test_workspace_crates_normalizes_on_insert() {
        let mut ws = WorkspaceCrates::default();
        ws.insert("my-lib".to_string());
        assert!(ws.contains("my_lib"), "should find normalized name");
        assert!(
            ws.contains("my-lib"),
            "should find hyphenated name via normalization"
        );
    }

    #[test]
    fn test_workspace_crates_from_iter_normalizes() {
        let ws: WorkspaceCrates = ["core-utils", "my-lib"].into_iter().collect();
        assert!(ws.contains("core_utils"));
        assert!(ws.contains("core-utils"));
        assert!(ws.contains("my_lib"));
        assert!(ws.contains("my-lib"));
    }

    #[test]
    fn test_workspace_crates_iter_returns_normalized() {
        let ws: WorkspaceCrates = ["core-utils"].into_iter().collect();
        let names: Vec<&str> = ws.iter().map(|s| s.as_str()).collect();
        assert_eq!(names, vec!["core_utils"]);
    }

    #[test]
    fn test_workspace_crates_len_and_is_empty() {
        let empty = WorkspaceCrates::default();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let ws: WorkspaceCrates = ["a", "b"].into_iter().collect();
        assert!(!ws.is_empty());
        assert_eq!(ws.len(), 2);
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
