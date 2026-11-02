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

/// The module paths of one package (e.g. `{"analyze", "analyze::hir"}`), and
/// which of them a `mod` with a body declares inside the file of its parent.
#[derive(Debug, Default, Clone)]
pub struct ModulePaths {
    pub paths: HashSet<String>,
    pub inline: HashSet<String>,
}

/// Package name → set of module paths (e.g. `{"analyze", "analyze::hir"}`),
/// inline modules among them.
#[derive(Debug, Default, Clone)]
pub struct ModulePathMap {
    paths: HashMap<String, HashSet<String>>,
    inline: HashMap<String, HashSet<String>>,
}

impl ModulePathMap {
    /// Return the module paths for a package, or an empty set if unknown.
    #[must_use]
    pub fn get_or_empty(&self, key: &str) -> &HashSet<String> {
        static EMPTY: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);
        self.paths.get(key).unwrap_or(&EMPTY)
    }

    /// Whether package `key` has the module `module`, its root (`""`)
    /// included.
    #[must_use]
    pub fn holds(&self, key: &str, module: &str) -> bool {
        module.is_empty() || self.get_or_empty(key).contains(module)
    }

    /// The module of package `key` whose file holds `module`: `module` itself
    /// unless it is declared inline, otherwise the nearest ancestor that is
    /// not.
    #[must_use]
    pub fn file_module<'m>(&self, key: &str, module: &'m str) -> &'m str {
        let Some(inline) = self.inline.get(key) else {
            return module;
        };
        let mut module = module;
        while inline.contains(module) {
            module = module.rsplit_once("::").map_or("", |(parent, _)| parent);
        }
        module
    }

    /// Copy this map with the module paths of `key` set to `modules`.
    #[must_use]
    pub fn with(&self, key: &str, modules: ModulePaths) -> Self {
        let mut map = self.clone();
        map.paths.insert(key.to_string(), modules.paths);
        map.inline.insert(key.to_string(), modules.inline);
        map
    }
}

impl Deref for ModulePathMap {
    type Target = HashMap<String, HashSet<String>>;
    fn deref(&self) -> &Self::Target {
        &self.paths
    }
}

impl FromIterator<(String, HashSet<String>)> for ModulePathMap {
    fn from_iter<I: IntoIterator<Item = (String, HashSet<String>)>>(iter: I) -> Self {
        Self {
            paths: iter.into_iter().collect(),
            inline: HashMap::new(),
        }
    }
}

impl FromIterator<(String, ModulePaths)> for ModulePathMap {
    fn from_iter<I: IntoIterator<Item = (String, ModulePaths)>>(iter: I) -> Self {
        let mut map = Self::default();
        for (key, modules) in iter {
            map.paths.insert(key.clone(), modules.paths);
            map.inline.insert(key, modules.inline);
        }
        map
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

    #[test]
    fn test_normalize_crate_name() {
        assert_eq!(normalize_crate_name("my-lib"), "my_lib");
        assert_eq!(normalize_crate_name("already_valid"), "already_valid");
        assert_eq!(normalize_crate_name("a-b-c"), "a_b_c");
    }
}
