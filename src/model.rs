//! Shared Data Structures
//!
//! Types used across analyze and graph modules, extracted to break circular dependencies.

use cargo_metadata::DependencyKind;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SourceLocation {
    /// Relative to the workspace root; a file outside it stays absolute.
    pub file: PathBuf,
    pub line: usize,
    pub symbols: Vec<String>,
    pub module_path: String,
    /// True when every reference at this location is a `pub use` re-export
    /// (republish, not behavioral use). Drives the logic-subgraph exclusion.
    pub via_reexport: bool,
}

/// Which dependency edge, as the crate-qualified names of its endpoints.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

impl Edge {
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// The distinct symbols crossing one module-dependency edge.
///
/// Counting locations instead would read an import group or a glob as a single
/// reference, no matter how many symbols it carries: one line is one location.
/// A reference the resolver cannot name sets `bare` rather than adding a name,
/// so such an edge never reads as free.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeSymbols {
    /// Sorted because the baseline file writes these names in iteration order.
    pub named: BTreeSet<String>,
    pub bare: bool,
}

impl EdgeSymbols {
    #[must_use]
    pub fn from_locations(locations: &[SourceLocation]) -> Self {
        Self {
            named: locations
                .iter()
                .flat_map(|l| l.symbols.iter().cloned())
                .collect(),
            bare: locations.iter().any(|l| l.symbols.is_empty()),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.named.len() + usize::from(self.bare)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.named.is_empty() && !self.bare
    }

    /// Whether `self` is at least as wide as `observed`, on names and on `bare`.
    #[must_use]
    pub fn covers(&self, observed: &Self) -> bool {
        observed.named.is_subset(&self.named) && (self.bare || !observed.bare)
    }

    #[must_use]
    pub fn difference(&self, other: &Self) -> Self {
        Self {
            named: self.named.difference(&other.named).cloned().collect(),
            bare: self.bare && !other.bare,
        }
    }

    /// Take in `other`'s symbols, so two records of one edge tolerate both sets.
    pub fn merge(&mut self, other: &Self) {
        self.named.extend(other.named.iter().cloned());
        self.bare |= other.bare;
    }
}

/// The item kind at a symbol's definition site.
///
/// Named apart from `layout::ItemKind`, which classifies graph nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Const,
    Static,
    Type,
}

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
    pub fn insert(&mut self, name: &str) -> bool {
        self.0.insert(normalize_crate_name(name))
    }

    /// Check membership, normalizing the input name.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(&normalize_crate_name(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
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

impl ModulePathMap {
    /// Return the module paths for a crate, or an empty set if unknown.
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
pub enum UsageKind {
    Production,
    Test(TestKind),
    Build,
}

impl UsageKind {
    #[must_use]
    pub fn kind_js(&self) -> &str {
        match self {
            Self::Production => "production",
            Self::Test(_) => "test",
            Self::Build => "build",
        }
    }

    #[must_use]
    pub fn sub_kind_js(&self) -> Option<&str> {
        match self {
            Self::Test(TestKind::Unit) => Some("unit"),
            Self::Test(TestKind::Integration) => Some("integration"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeContext {
    pub kind: UsageKind,
    pub features: Vec<String>,
}

impl EdgeContext {
    #[must_use]
    pub fn production() -> Self {
        Self {
            kind: UsageKind::Production,
            features: vec![],
        }
    }
    #[must_use]
    pub fn test(kind: TestKind) -> Self {
        Self {
            kind: UsageKind::Test(kind),
            features: vec![],
        }
    }
    #[must_use]
    pub fn build() -> Self {
        Self {
            kind: UsageKind::Build,
            features: vec![],
        }
    }
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
    pub workspace_root: PathBuf,
    pub target_roots: TargetRoots,
    pub manifest: PathBuf,
    pub dependencies: Vec<String>,
    /// Populated regardless of `--include-tests`. Reachability needs to know
    /// who is pulled in by tests even when the view does not show it.
    pub dev_dependencies: Vec<String>,
}

/// Source files of a package's lib and bin targets, absolute, as Cargo
/// resolved them. Workspace crates and external packages share the shape.
#[derive(Debug, Clone, Default)]
pub struct TargetRoots {
    /// `None` for binary-only crates. Cargo lets `[lib] path` point anywhere,
    /// so this is not always `src/lib.rs`.
    pub lib_root: Option<PathBuf>,
    /// Likewise not always `src/main.rs`.
    pub bin_roots: Vec<PathBuf>,
}

impl TargetRoots {
    /// Iterate the entry points for module walking, library target first.
    pub fn files(&self) -> impl Iterator<Item = &Path> {
        self.lib_root
            .iter()
            .chain(&self.bin_roots)
            .map(PathBuf::as_path)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DependencyRef {
    pub target_crate: String,
    pub target_module: String,
    pub target_item: Option<String>,
    /// Relative to the workspace root; a file outside it stays absolute.
    pub source_file: PathBuf,
    pub line: usize,
    pub context: EdgeContext,
    /// True when this reference stems from a `pub use` re-export (republish),
    /// not a behavioral use. Stamped in `parse_workspace_dependencies`.
    pub via_reexport: bool,
}

impl DependencyRef {
    /// Returns full target path: "`crate::module::item`" or "`crate::module`" if no item.
    /// For empty `target_module` (crate root): "`crate::item`" or just "crate".
    #[must_use]
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

    /// Returns module-level target: "`crate::module`" (ignores item).
    /// For empty `target_module` (crate root): just "crate".
    #[must_use]
    pub fn module_target(&self) -> String {
        if self.target_module.is_empty() {
            self.target_crate.clone()
        } else {
            format!("{}::{}", self.target_crate, self.target_module)
        }
    }

    /// Build a lookup index from an existing slice of dependencies.
    /// Maps `(full_target, kind)` to the position in the slice.
    pub(crate) fn build_seen_index(deps: &[DependencyRef]) -> HashMap<(String, UsageKind), usize> {
        deps.iter()
            .enumerate()
            .map(|(i, d)| ((d.full_target(), d.context.kind), i))
            .collect()
    }

    /// Insert a dependency, deduplicating by `(full_target, kind)`.
    /// If a duplicate exists, merges features into the existing entry.
    pub(crate) fn dedup_push(
        deps: &mut Vec<DependencyRef>,
        seen: &mut HashMap<(String, UsageKind), usize>,
        dep: DependencyRef,
    ) {
        let key = (dep.full_target(), dep.context.kind);
        if let Some(&idx) = seen.get(&key) {
            deps[idx].context.features.extend(dep.context.features);
            // Re-export only if every merged reference is a re-export.
            deps[idx].via_reexport &= dep.via_reexport;
        } else {
            seen.insert(key, deps.len());
            deps.push(dep);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub full_path: String,
    /// Absolute path of the file that declares this module. `None` for the
    /// root of a crate without a lib or bin target, which has no file, and
    /// under the `hir` backend for a module whose VFS path is not on disk.
    pub file: Option<PathBuf>,
    pub children: Vec<ModuleInfo>,
    pub dependencies: Vec<DependencyRef>,
}

#[derive(Debug, Clone)]
pub struct ModuleTree {
    pub root: ModuleInfo,
}

/// Metadata for a single external crate (one entry per version).
#[derive(Debug, Clone)]
pub(crate) struct ExternalCrateInfo {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) package_id: String,
    /// Inside the registry or vendor directory.
    pub(crate) target_roots: TargetRoots,
    pub(crate) manifest: PathBuf,
}

/// Dependency edge between two external crates.
#[derive(Debug, Clone)]
pub(crate) struct ExternalDep {
    pub(crate) from_pkg_id: String,
    pub(crate) to_pkg_id: String,
    pub(crate) dep_kinds: Vec<DependencyKind>,
}

/// Dependency edge from a workspace crate to an external crate.
#[derive(Debug, Clone)]
pub(crate) struct WorkspaceExternalDep {
    pub(crate) workspace_crate: String,
    pub(crate) external_pkg_id: String,
    pub(crate) dep_kinds: Vec<DependencyKind>,
}

/// Result of external dependency analysis from cargo metadata.
#[derive(Debug)]
pub(crate) struct ExternalsResult {
    pub(crate) crates: Vec<ExternalCrateInfo>,
    pub(crate) external_deps: Vec<ExternalDep>,
    pub(crate) workspace_deps: Vec<WorkspaceExternalDep>,
    /// `workspace_crate_name` -> (`code_name` -> `package_id`).
    /// Per-workspace-crate map because different workspace crates can depend on
    /// different versions of the same external crate.
    pub(crate) crate_name_map: HashMap<String, HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn location(symbols: &[&str]) -> SourceLocation {
        SourceLocation {
            file: PathBuf::from("src/a.rs"),
            line: 1,
            symbols: symbols.iter().map(|s| (*s).to_string()).collect(),
            module_path: "krate::a".to_string(),
            via_reexport: false,
        }
    }

    #[test]
    fn import_group_counts_every_symbol_it_carries() {
        let symbols = EdgeSymbols::from_locations(&[location(&["One", "Two", "Three"])]);
        assert_eq!(symbols.len(), 3);
    }

    #[test]
    fn unnamed_references_share_one_slot() {
        let symbols = EdgeSymbols::from_locations(&[location(&[]), location(&[])]);
        assert_eq!(symbols.len(), 1);
    }

    #[test]
    fn the_same_symbol_on_two_lines_counts_once() {
        let symbols = EdgeSymbols::from_locations(&[location(&["One"]), location(&["One", "Two"])]);
        assert_eq!(symbols.len(), 2);
    }

    #[test]
    fn a_wider_entry_covers_a_narrower_observation() {
        let frozen = EdgeSymbols::from_locations(&[location(&["Foo", "Bar"])]);
        assert!(frozen.covers(&EdgeSymbols::from_locations(&[location(&["Foo"])])));
        assert!(!frozen.covers(&EdgeSymbols::from_locations(&[location(&["Foo", "Baz"])])));
    }

    #[test]
    fn an_unnamed_reference_needs_the_bare_flag_to_be_covered() {
        let named_only = EdgeSymbols::from_locations(&[location(&["Foo"])]);
        let bare = EdgeSymbols::from_locations(&[location(&[])]);
        assert!(!named_only.covers(&bare));

        let with_bare = EdgeSymbols::from_locations(&[location(&["Foo"]), location(&[])]);
        assert!(with_bare.covers(&bare));
    }

    #[test]
    fn test_edgecontext_struct_basics() {
        let ctx = EdgeContext::production();
        assert_eq!(ctx.kind, UsageKind::Production);
        assert!(ctx.features.is_empty());
        let test_ctx = EdgeContext::test(TestKind::Unit);
        assert_eq!(test_ctx.kind, UsageKind::Test(TestKind::Unit));
        assert_ne!(ctx, test_ctx);
        let cloned = ctx.clone();
        assert_eq!(ctx, cloned);
    }

    #[test]
    fn test_dependency_kind_js_strings() {
        assert_eq!(UsageKind::Production.kind_js(), "production");
        assert_eq!(UsageKind::Production.sub_kind_js(), None);

        assert_eq!(UsageKind::Test(TestKind::Unit).kind_js(), "test");
        assert_eq!(UsageKind::Test(TestKind::Unit).sub_kind_js(), Some("unit"));

        assert_eq!(UsageKind::Test(TestKind::Integration).kind_js(), "test");
        assert_eq!(
            UsageKind::Test(TestKind::Integration).sub_kind_js(),
            Some("integration")
        );

        assert_eq!(UsageKind::Build.kind_js(), "build");
        assert_eq!(UsageKind::Build.sub_kind_js(), None);
    }

    #[test]
    fn test_dependency_kind_is_copy_and_hash() {
        use std::collections::HashSet;
        let a = UsageKind::Production;
        let b = a; // Copy
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(UsageKind::Test(TestKind::Unit));
        set.insert(UsageKind::Build);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_dependency_ref_carries_context() {
        let prod_dep = DependencyRef {
            target_crate: "my_crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::from("src/lib.rs"),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
        };
        assert_eq!(prod_dep.context, EdgeContext::production());

        let test_dep = DependencyRef {
            target_crate: "my_crate".to_string(),
            target_module: "graph".to_string(),
            target_item: None,
            source_file: PathBuf::from("src/lib.rs"),
            line: 1,
            context: EdgeContext::test(TestKind::Unit),
            via_reexport: false,
        };
        assert_eq!(test_dep.context, EdgeContext::test(TestKind::Unit));

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
            context: EdgeContext::production(),
            via_reexport: false,
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
            context: EdgeContext::production(),
            via_reexport: false,
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
            context: EdgeContext::production(),
            via_reexport: false,
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
            context: EdgeContext::production(),
            via_reexport: false,
        };
        assert_eq!(dep.full_target(), "crate::graph");
    }

    #[test]
    fn test_module_target_empty_module() {
        let dep = DependencyRef {
            target_crate: "crate_b".to_string(),
            target_module: String::new(),
            target_item: None,
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
        };
        assert_eq!(dep.module_target(), "crate_b");
    }

    #[test]
    fn test_full_target_empty_module_with_item() {
        let dep = DependencyRef {
            target_crate: "crate_b".to_string(),
            target_module: String::new(),
            target_item: Some("Symbol".to_string()),
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
        };
        assert_eq!(dep.full_target(), "crate_b::Symbol");
    }

    #[test]
    fn test_full_target_empty_module_no_item() {
        let dep = DependencyRef {
            target_crate: "crate_b".to_string(),
            target_module: String::new(),
            target_item: None,
            source_file: PathBuf::new(),
            line: 1,
            context: EdgeContext::production(),
            via_reexport: false,
        };
        assert_eq!(dep.full_target(), "crate_b");
    }

    #[test]
    fn test_workspace_crates_normalizes_on_insert() {
        let mut ws = WorkspaceCrates::default();
        ws.insert("my-lib");
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
        let names: Vec<&str> = ws.iter().map(std::string::String::as_str).collect();
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
            file: None,
            children: vec![],
            dependencies: vec![DependencyRef {
                target_crate: "crate".to_string(),
                target_module: "graph".to_string(),
                target_item: None,
                source_file: PathBuf::from("src/cli.rs"),
                line: 5,
                context: EdgeContext::production(),
                via_reexport: false,
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
