//! Lookups keyed by normalized crate name, shared by the Rust analysis stages.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;

/// Workspace crate names, stored in normalized form (hyphens → underscores).
///
/// All insertion paths normalize names, and `contains()` normalizes its input,
/// so lookups are O(1) and callers never need to think about normalization.
#[derive(Debug, Default, Clone)]
pub struct WorkspacePackages(HashSet<String>);

impl FromIterator<String> for WorkspacePackages {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self(iter.into_iter().map(|s| normalize_crate_name(&s)).collect())
    }
}

impl<'a> FromIterator<&'a str> for WorkspacePackages {
    fn from_iter<I: IntoIterator<Item = &'a str>>(iter: I) -> Self {
        Self(iter.into_iter().map(normalize_crate_name).collect())
    }
}

impl WorkspacePackages {
    /// Check membership, normalizing the input name.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(&normalize_crate_name(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }
}

pub(crate) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Package name → set of module paths (e.g. `{"analyze", "analyze::hir"}`).
#[derive(Debug, Default, Clone)]
pub struct ModulePathMap(HashMap<String, HashSet<String>>);

impl ModulePathMap {
    /// Return the module paths for a package, or an empty set if unknown.
    #[must_use]
    pub fn get_or_empty(&self, key: &str) -> &HashSet<String> {
        static EMPTY: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);
        self.0.get(key).unwrap_or(&EMPTY)
    }
}

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

/// Package name → set of exported symbol names.
#[derive(Debug, Default, Clone)]
pub struct PackageExportMap(HashMap<String, HashSet<String>>);

impl Deref for PackageExportMap {
    type Target = HashMap<String, HashSet<String>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<(String, HashSet<String>)> for PackageExportMap {
    fn from_iter<I: IntoIterator<Item = (String, HashSet<String>)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_packages_from_iter_normalizes() {
        let ws: WorkspacePackages = ["core-utils", "my-lib"].into_iter().collect();
        assert!(ws.contains("core_utils"));
        assert!(ws.contains("core-utils"));
        assert!(ws.contains("my_lib"));
        assert!(ws.contains("my-lib"));
    }

    #[test]
    fn test_workspace_packages_iter_returns_normalized() {
        let ws: WorkspacePackages = ["core-utils"].into_iter().collect();
        let names: Vec<&str> = ws.iter().map(std::string::String::as_str).collect();
        assert_eq!(names, vec!["core_utils"]);
    }
}
