//! Syn-based use statement parsing for workspace dependency extraction.

use crate::model::DependencyRef;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use syn::UseTree;
use syn::visit::Visit;

/// Collect all `use` items from a parsed file, including those nested inside
/// function bodies, blocks, and other scopes. Uses `syn::visit::Visit` to
/// traverse the full AST regardless of nesting depth.
pub(crate) fn collect_all_use_items(syntax: &syn::File) -> Vec<syn::ItemUse> {
    struct UseCollector {
        uses: Vec<syn::ItemUse>,
    }
    impl<'ast> Visit<'ast> for UseCollector {
        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            self.uses.push(node.clone());
        }
    }
    let mut collector = UseCollector { uses: Vec::new() };
    collector.visit_file(syntax);
    collector.uses
}

/// Recursively resolve a `syn::UseTree` into fully-qualified path strings.
///
/// Example: `use cli::{Args, Cargo, run}` → `["cli::Args", "cli::Cargo", "cli::run"]`
fn resolve_use_tree(tree: &UseTree, prefix: &str) -> Vec<String> {
    match tree {
        UseTree::Path(p) => {
            let segment = p.ident.to_string();
            let new_prefix = if prefix.is_empty() {
                segment
            } else {
                format!("{prefix}::{segment}")
            };
            resolve_use_tree(&p.tree, &new_prefix)
        }
        UseTree::Name(n) => {
            let name = n.ident.to_string();
            if prefix.is_empty() {
                vec![name]
            } else {
                vec![format!("{prefix}::{name}")]
            }
        }
        UseTree::Rename(r) => {
            // Use original name, not alias — we track the *source* dependency
            let name = r.ident.to_string();
            if prefix.is_empty() {
                vec![name]
            } else {
                vec![format!("{prefix}::{name}")]
            }
        }
        UseTree::Glob(_) => {
            if prefix.is_empty() {
                vec!["*".to_string()]
            } else {
                vec![format!("{prefix}::*")]
            }
        }
        UseTree::Group(g) => g
            .items
            .iter()
            .flat_map(|item| resolve_use_tree(item, prefix))
            .collect(),
    }
}

pub(crate) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Find the longest module path prefix from `parts` that exists in `module_paths`.
///
/// Tries from longest to shortest: `["analyze", "use_parser", "normalize"]`
/// checks `"analyze::use_parser"`, then `"analyze"`.
/// Returns `(matched_path, segment_count)`.
/// Fallback: first segment with count 1.
fn find_longest_module_prefix(parts: &[&str], module_paths: &HashSet<String>) -> (String, usize) {
    for end in (1..=parts.len()).rev() {
        let candidate: String = parts[..end].join("::");
        if module_paths.contains(&candidate) {
            return (candidate, end);
        }
    }
    // Fallback: first segment
    (parts[0].to_string(), 1)
}

pub(super) fn is_workspace_member<S: AsRef<str>>(
    name: &str,
    workspace_crates: &HashSet<S>,
) -> bool {
    let normalized = normalize_crate_name(name);
    workspace_crates
        .iter()
        .any(|ws| normalize_crate_name(ws.as_ref()) == normalized)
}

/// Extract an item from path parts at given index, handling trailing `{` and empty strings.
fn extract_item_from_parts(parts: &[&str], index: usize) -> Option<String> {
    let part = parts
        .get(index)?
        .trim_end_matches('{')
        .trim_end_matches(';')
        .trim();
    if part.is_empty() || part.starts_with('{') {
        None
    } else {
        Some(part.to_string())
    }
}

/// Parse crate-local imports: `use crate::module[::item]`
fn parse_crate_local_import(
    path: &str,
    current_crate: &str,
    source_file: &Path,
    line_num: usize,
    all_module_paths: &HashMap<String, HashSet<String>>,
) -> Option<DependencyRef> {
    let after_crate = path.strip_prefix("crate::")?;
    let parts: Vec<&str> = after_crate.split("::").collect();

    let first = parts.first()?.trim_end_matches('{').trim();
    if first.is_empty() {
        return None;
    }

    let empty_set = HashSet::new();
    let module_paths = all_module_paths
        .get(&normalize_crate_name(current_crate))
        .unwrap_or(&empty_set);
    let (target_module, prefix_len) = find_longest_module_prefix(&parts, module_paths);

    Some(DependencyRef {
        target_crate: normalize_crate_name(current_crate),
        target_module,
        target_item: extract_item_from_parts(&parts, prefix_len),
        source_file: source_file.to_path_buf(),
        line: line_num,
    })
}

/// Parse bare module imports: `use cli::Args` where `cli` is a known module of the current crate.
/// Rust 2018+ resolves bare paths from any file, not just the crate root.
fn parse_bare_module_import(
    path: &str,
    current_crate: &str,
    source_file: &Path,
    line_num: usize,
    all_module_paths: &HashMap<String, HashSet<String>>,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let first = parts.first()?.trim_end_matches('{').trim();
    if first.is_empty() {
        return None;
    }

    let empty_set = HashSet::new();
    let module_paths = all_module_paths
        .get(&normalize_crate_name(current_crate))
        .unwrap_or(&empty_set);

    if !module_paths.contains(first) {
        return None;
    }

    let (target_module, prefix_len) = find_longest_module_prefix(&parts, module_paths);

    Some(DependencyRef {
        target_crate: normalize_crate_name(current_crate),
        target_module,
        target_item: extract_item_from_parts(&parts, prefix_len),
        source_file: source_file.to_path_buf(),
        line: line_num,
    })
}

/// Parse workspace crate imports: `use other_crate::module[::item]`
fn parse_workspace_import(
    path: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    line_num: usize,
    all_module_paths: &HashMap<String, HashSet<String>>,
    crate_exports: &HashMap<String, HashSet<String>>,
) -> Option<DependencyRef> {
    let parts: Vec<&str> = path.split("::").collect();
    let crate_name = parts.first()?.trim();

    if !is_workspace_member(crate_name, workspace_crates) || parts.len() < 2 {
        return None;
    }

    let module_segment = parts[1].trim_end_matches('{').trim_end_matches(';').trim();
    if module_segment.is_empty() {
        return None;
    }

    let empty_set = HashSet::new();
    let target_crate_name = normalize_crate_name(crate_name);
    let module_paths = all_module_paths
        .get(&target_crate_name)
        .unwrap_or(&empty_set);
    let (target_module, prefix_len) = find_longest_module_prefix(&parts[1..], module_paths);

    // Entry-point detection: if the resolved target_module is not a known module
    // and the first segment after the crate name is a known export, treat it as
    // an entry-point dependency (target_module = "").
    if !module_paths.contains(&target_module)
        && crate_exports
            .get(&target_crate_name)
            .is_some_and(|e| e.contains(module_segment))
    {
        return Some(DependencyRef {
            target_crate: crate_name.to_string(),
            target_module: String::new(),
            target_item: Some(module_segment.to_string()),
            source_file: source_file.to_path_buf(),
            line: line_num,
        });
    }

    Some(DependencyRef {
        target_crate: crate_name.to_string(),
        target_module,
        target_item: extract_item_from_parts(&parts, 1 + prefix_len),
        source_file: source_file.to_path_buf(),
        line: line_num,
    })
}

/// Resolve a single use path through the resolution chain: crate-local → bare module → workspace.
/// Handles glob paths (`crate::module::*`) by stripping the glob and setting target_item = "*".
fn resolve_single_path(
    path: &str,
    line_num: usize,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    all_module_paths: &HashMap<String, HashSet<String>>,
    crate_exports: &HashMap<String, HashSet<String>>,
) -> Option<DependencyRef> {
    // Handle glob: `crate::module::*` → resolve base, set target_item = "*"
    if let Some(base) = path.strip_suffix("::*") {
        let mut dep = resolve_single_path(
            base,
            line_num,
            current_crate,
            workspace_crates,
            source_file,
            all_module_paths,
            crate_exports,
        )?;
        // The base resolved as a module — push "*" as the item
        dep.target_item = Some("*".to_string());
        return Some(dep);
    }

    parse_crate_local_import(path, current_crate, source_file, line_num, all_module_paths)
        .or_else(|| {
            parse_bare_module_import(path, current_crate, source_file, line_num, all_module_paths)
        })
        .or_else(|| {
            parse_workspace_import(
                path,
                workspace_crates,
                source_file,
                line_num,
                all_module_paths,
                crate_exports,
            )
        })
        .or_else(|| {
            // Bare workspace crate name (e.g. from `use other_crate::{Foo}` → path = "other_crate")
            if !path.contains("::") && is_workspace_member(path, workspace_crates) {
                Some(DependencyRef {
                    target_crate: path.to_string(),
                    target_module: String::new(),
                    target_item: None,
                    source_file: source_file.to_path_buf(),
                    line: line_num,
                })
            } else {
                None
            }
        })
}

/// Parse syn-based use items, extracting workspace-relevant dependencies.
///
/// Returns DependencyRefs for:
/// - Crate-local imports (`use crate::module`)
/// - Workspace crate imports (`use other_crate::module` where other_crate is in workspace)
///
/// Deduplicates by full_target() to keep distinct symbols but avoid duplicates.
pub(crate) fn parse_workspace_dependencies(
    use_items: &[syn::ItemUse],
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    all_module_paths: &HashMap<String, HashSet<String>>,
    crate_exports: &HashMap<String, HashSet<String>>,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashSet<String> = HashSet::new();

    for item in use_items {
        let line_num = item.use_token.span.start().line;
        let paths = resolve_use_tree(&item.tree, "");

        for path in paths {
            if let Some(dep) = resolve_single_path(
                &path,
                line_num,
                current_crate,
                workspace_crates,
                source_file,
                all_module_paths,
                crate_exports,
            ) {
                let target_key = dep.full_target();
                if !seen_targets.contains(&target_key) {
                    seen_targets.insert(target_key);
                    deps.push(dep);
                }
            }
        }
    }

    deps
}

/// Convenience wrapper: parse source text into syn::ItemUse items and extract dependencies.
/// Used by hir.rs which has source text but no pre-parsed AST.
#[cfg(feature = "hir")]
pub(crate) fn parse_workspace_dependencies_from_source(
    source: &str,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    all_module_paths: &HashMap<String, HashSet<String>>,
    crate_exports: &HashMap<String, HashSet<String>>,
) -> Vec<DependencyRef> {
    let syntax = match syn::parse_file(source) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let uses = collect_all_use_items(&syntax);
    parse_workspace_dependencies(
        &uses,
        current_crate,
        workspace_crates,
        source_file,
        all_module_paths,
        crate_exports,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse_test_uses(source: &str) -> Vec<syn::ItemUse> {
        collect_all_use_items(&syn::parse_file(source).unwrap())
    }

    mod normalize_tests {
        use super::*;

        #[test]
        fn test_normalize_crate_name() {
            assert_eq!(normalize_crate_name("my-lib"), "my_lib");
            assert_eq!(normalize_crate_name("already_valid"), "already_valid");
            assert_eq!(normalize_crate_name("a-b-c"), "a_b_c");
        }

        #[test]
        fn test_process_use_statement_crate_local() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/cli.rs");
            let uses = parse_test_uses("use crate::graph::build;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "graph");
            assert_eq!(dep.target_item, Some("build".to_string()));
        }

        #[test]
        fn test_process_use_statement_crate_local_module_only() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use crate::graph;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "graph");
            assert!(dep.target_item.is_none());
            assert_eq!(dep.line, 1);
        }

        #[test]
        fn test_process_use_statement_workspace_crate() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::utils;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_crate, "other_crate");
            assert_eq!(dep.target_module, "utils");
        }

        #[test]
        fn test_process_use_statement_workspace_crate_with_hyphen() {
            let ws: HashSet<String> = HashSet::from(["my-lib".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/main.rs");
            let uses = parse_test_uses("use my_lib::feature;");
            let deps = parse_workspace_dependencies(&uses, "app", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_crate, "my_lib");
            assert_eq!(dep.target_module, "feature");
        }

        #[test]
        fn test_process_use_statement_relative_self_ignored() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use self::helper;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert!(deps.is_empty(), "self:: imports should be ignored");
        }

        #[test]
        fn test_process_use_statement_relative_super_ignored() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/sub/mod.rs");
            let uses = parse_test_uses("use super::parent;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert!(deps.is_empty(), "super:: imports should be ignored");
        }

        #[test]
        fn test_process_use_statement_external_filtered() {
            let ws: HashSet<String> = HashSet::from(["my_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use serde::Serialize;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert!(deps.is_empty(), "external crate imports should be filtered");
        }

        #[test]
        fn test_process_use_statement_std_filtered() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use std::collections::HashMap;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert!(deps.is_empty(), "std imports should be filtered");
        }
    }

    mod longest_prefix_tests {
        use super::*;

        #[test]
        fn test_find_longest_prefix_submodule() {
            let paths: HashSet<String> =
                HashSet::from(["analyze".into(), "analyze::use_parser".into()]);
            let (prefix, len) =
                find_longest_module_prefix(&["analyze", "use_parser", "normalize"], &paths);
            assert_eq!(prefix, "analyze::use_parser");
            assert_eq!(len, 2);
        }

        #[test]
        fn test_find_longest_prefix_parent_only() {
            let paths: HashSet<String> = HashSet::from(["analyze".into()]);
            let (prefix, len) = find_longest_module_prefix(&["analyze", "SomeItem"], &paths);
            assert_eq!(prefix, "analyze");
            assert_eq!(len, 1);
        }

        #[test]
        fn test_find_longest_prefix_no_match() {
            let paths: HashSet<String> = HashSet::from(["analyze".into()]);
            let (prefix, len) = find_longest_module_prefix(&["unknown", "item"], &paths);
            assert_eq!(prefix, "unknown");
            assert_eq!(len, 1);
        }

        #[test]
        fn test_find_longest_prefix_single_segment() {
            let paths: HashSet<String> = HashSet::from(["graph".into()]);
            let (prefix, len) = find_longest_module_prefix(&["graph"], &paths);
            assert_eq!(prefix, "graph");
            assert_eq!(len, 1);
        }

        #[test]
        fn test_find_longest_prefix_empty_module_paths() {
            let paths: HashSet<String> = HashSet::new();
            let (prefix, len) = find_longest_module_prefix(&["analyze", "foo"], &paths);
            assert_eq!(prefix, "analyze");
            assert_eq!(len, 1);
        }
    }

    mod submodule_tests {
        use super::*;

        #[test]
        fn test_crate_local_submodule() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "my_crate".to_string(),
                HashSet::from(["analyze".into(), "analyze::use_parser".into()]),
            )]);
            let path = Path::new("src/cli.rs");
            let uses = parse_test_uses("use crate::analyze::use_parser::normalize;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_module, "analyze::use_parser");
            assert_eq!(dep.target_item, Some("normalize".to_string()));
        }

        #[test]
        fn test_workspace_import_submodule() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "other_crate".to_string(),
                HashSet::from(["foo".into(), "foo::bar".into()]),
            )]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::foo::bar::Baz;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_module, "foo::bar");
            assert_eq!(dep.target_item, Some("Baz".to_string()));
        }

        #[test]
        fn test_workspace_import_cross_crate_deep() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "other_crate".to_string(),
                HashSet::from(["sub".into(), "sub::deep".into()]),
            )]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::sub::deep::Item;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_module, "sub::deep");
            assert_eq!(dep.target_item, Some("Item".to_string()));
        }

        #[test]
        fn test_cross_crate_no_paths_fallback() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::foo::bar::Baz;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_module, "foo");
            assert_eq!(dep.target_item, Some("bar".to_string()));
        }

        #[test]
        fn test_multi_symbol_with_submodule() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "my_crate".to_string(),
                HashSet::from(["analyze".into(), "analyze::use_parser".into()]),
            )]);
            let path = Path::new("src/cli.rs");
            let uses = parse_test_uses(
                "use crate::analyze::use_parser::{normalize, is_workspace_member};",
            );
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 2, "should return 2 deps: {:?}", deps);
            assert_eq!(deps[0].target_module, "analyze::use_parser");
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("normalize".to_string()))
            );
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("is_workspace_member".to_string()))
            );
        }
    }

    mod parsing_tests {
        use super::*;

        #[test]
        fn test_parse_workspace_dependencies_mixed() {
            let source = r#"
use crate::graph;
use other_crate::utils;
use serde::Serialize;
use std::collections::HashMap;
"#;
            let ws: HashSet<String> = HashSet::from(["my_crate".into(), "other_crate".into()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let uses = parse_test_uses(source);
            let deps = parse_workspace_dependencies(
                &uses,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
                &HashMap::new(),
            );

            assert_eq!(deps.len(), 2, "found: {:?}", deps);
            assert!(
                deps.iter()
                    .any(|d| d.target_crate == "my_crate" && d.target_module == "graph")
            );
            assert!(
                deps.iter()
                    .any(|d| d.target_crate == "other_crate" && d.target_module == "utils")
            );
        }

        #[test]
        fn test_parse_workspace_dependencies_dedup_by_full_target() {
            let source = r#"
use crate::graph::build;
use crate::graph::Node;
use crate::graph;
"#;
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let uses = parse_test_uses(source);
            let deps = parse_workspace_dependencies(
                &uses,
                "my_crate",
                &ws,
                Path::new("src/cli.rs"),
                &mp,
                &HashMap::new(),
            );

            assert_eq!(deps.len(), 3, "should keep distinct symbols: {:?}", deps);
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("build".to_string()))
            );
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("Node".to_string()))
            );
            assert!(deps.iter().any(|d| d.target_item.is_none()));
        }

        #[test]
        fn test_process_use_multi_symbol() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/cli.rs");
            let uses = parse_test_uses("use crate::graph::{Node, Edge};");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 2, "should return 2 deps: {:?}", deps);
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("Node".to_string()))
            );
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("Edge".to_string()))
            );
        }

        #[test]
        fn test_process_use_glob() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/cli.rs");
            let uses = parse_test_uses("use crate::analyze::*;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1, "glob should return 1 dep: {:?}", deps);
            assert_eq!(deps[0].target_item, Some("*".to_string()));
            assert_eq!(deps[0].target_module, "analyze");
        }
    }

    mod integration_tests {
        use super::*;

        #[test]
        fn test_bare_module_pub_use_integration() {
            let source = r#"
pub use cli::{Args, Cargo, run};
use crate::graph::build;
pub(crate) use model::Node;
use other_crate::utils;
use serde::Serialize;
"#;
            let ws: HashSet<String> = HashSet::from(["my_crate".into(), "other_crate".into()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "my_crate".to_string(),
                HashSet::from(["cli".into(), "graph".into(), "model".into()]),
            )]);
            let uses = parse_test_uses(source);
            let deps = parse_workspace_dependencies(
                &uses,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
                &HashMap::new(),
            );

            // Expected: 6 DependencyRefs
            // - cli::Args, cli::Cargo, cli::run (bare module, pub use multi)
            // - graph::build (crate:: prefix)
            // - model::Node (bare module, pub(crate))
            // - other_crate::utils (workspace crate)
            assert_eq!(deps.len(), 6, "expected 6 deps, got: {:?}", deps);

            // Bare module multi-import (pub use)
            assert!(
                deps.iter().any(|d| d.target_crate == "my_crate"
                    && d.target_module == "cli"
                    && d.target_item == Some("Args".to_string())),
                "missing cli::Args"
            );
            assert!(
                deps.iter().any(|d| d.target_crate == "my_crate"
                    && d.target_module == "cli"
                    && d.target_item == Some("Cargo".to_string())),
                "missing cli::Cargo"
            );
            assert!(
                deps.iter().any(|d| d.target_crate == "my_crate"
                    && d.target_module == "cli"
                    && d.target_item == Some("run".to_string())),
                "missing cli::run"
            );

            // crate:: prefix
            assert!(
                deps.iter().any(|d| d.target_crate == "my_crate"
                    && d.target_module == "graph"
                    && d.target_item == Some("build".to_string())),
                "missing graph::build"
            );

            // Bare module pub(crate)
            assert!(
                deps.iter().any(|d| d.target_crate == "my_crate"
                    && d.target_module == "model"
                    && d.target_item == Some("Node".to_string())),
                "missing model::Node"
            );

            // Workspace crate
            assert!(
                deps.iter()
                    .any(|d| d.target_crate == "other_crate" && d.target_module == "utils"),
                "missing other_crate::utils"
            );
        }
    }

    mod bare_module_tests {
        use super::*;

        #[test]
        fn test_bare_module_simple() {
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("my_crate".to_string(), HashSet::from(["cli".into()]))]);
            let dep =
                parse_bare_module_import("cli::Args", "my_crate", Path::new("src/lib.rs"), 1, &mp);
            let dep = dep.expect("should parse bare module import");
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "cli");
            assert_eq!(dep.target_item, Some("Args".to_string()));
        }

        #[test]
        fn test_bare_module_no_match() {
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("my_crate".to_string(), HashSet::from(["cli".into()]))]);
            let dep = parse_bare_module_import(
                "serde::Serialize",
                "my_crate",
                Path::new("src/lib.rs"),
                1,
                &mp,
            );
            assert!(dep.is_none(), "external crate should not match");
        }

        #[test]
        fn test_bare_module_deep_path() {
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "my_crate".to_string(),
                HashSet::from(["analyze".into(), "analyze::use_parser".into()]),
            )]);
            let dep = parse_bare_module_import(
                "analyze::use_parser::normalize",
                "my_crate",
                Path::new("src/lib.rs"),
                1,
                &mp,
            );
            let dep = dep.expect("should parse deep bare module import");
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "analyze::use_parser");
            assert_eq!(dep.target_item, Some("normalize".to_string()));
        }

        #[test]
        fn test_bare_module_module_only() {
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("my_crate".to_string(), HashSet::from(["cli".into()]))]);
            let dep = parse_bare_module_import("cli", "my_crate", Path::new("src/lib.rs"), 1, &mp);
            let dep = dep.expect("should parse module-only bare import");
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "cli");
            assert!(dep.target_item.is_none());
        }

        #[test]
        fn test_bare_module_via_parse_workspace_dependencies() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("my_crate".to_string(), HashSet::from(["cli".into()]))]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use cli::Args;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "cli");
            assert_eq!(dep.target_item, Some("Args".to_string()));
        }

        #[test]
        fn test_bare_module_pub_use_via_parse_workspace_dependencies() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("my_crate".to_string(), HashSet::from(["cli".into()]))]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("pub(crate) use cli::Args;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "cli");
            assert_eq!(dep.target_item, Some("Args".to_string()));
        }

        #[test]
        fn test_bare_module_multi_import() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("my_crate".to_string(), HashSet::from(["cli".into()]))]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use cli::{Args, Cargo, run};");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 3, "should return 3 deps: {:?}", deps);
            assert!(deps.iter().all(|d| d.target_crate == "my_crate"));
            assert!(deps.iter().all(|d| d.target_module == "cli"));
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("Args".to_string()))
            );
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("Cargo".to_string()))
            );
            assert!(
                deps.iter()
                    .any(|d| d.target_item == Some("run".to_string()))
            );
        }

        #[test]
        fn test_bare_module_glob_import() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("my_crate".to_string(), HashSet::from(["cli".into()]))]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use cli::*;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1, "glob should return 1 dep: {:?}", deps);
            assert_eq!(deps[0].target_crate, "my_crate");
            assert_eq!(deps[0].target_module, "cli");
            assert_eq!(deps[0].target_item, Some("*".to_string()));
        }
    }

    mod resolve_use_tree_tests {
        use super::*;

        fn parse_use_tree(source: &str) -> syn::UseTree {
            let file: syn::File = syn::parse_str(source).unwrap();
            match file.items.into_iter().next().unwrap() {
                syn::Item::Use(u) => u.tree,
                _ => panic!("expected use item"),
            }
        }

        #[test]
        fn test_simple_path() {
            let tree = parse_use_tree("use crate::graph::build;");
            let paths = resolve_use_tree(&tree, "");
            assert_eq!(paths, vec!["crate::graph::build"]);
        }

        #[test]
        fn test_multi_import() {
            let tree = parse_use_tree("use cli::{Args, Cargo};");
            let mut paths = resolve_use_tree(&tree, "");
            paths.sort();
            assert_eq!(paths, vec!["cli::Args", "cli::Cargo"]);
        }

        #[test]
        fn test_glob() {
            let tree = parse_use_tree("use model::*;");
            let paths = resolve_use_tree(&tree, "");
            assert_eq!(paths, vec!["model::*"]);
        }

        #[test]
        fn test_rename() {
            let tree = parse_use_tree("use cli::Args as CliArgs;");
            let paths = resolve_use_tree(&tree, "");
            assert_eq!(paths, vec!["cli::Args"]);
        }

        #[test]
        fn test_nested_groups() {
            let tree = parse_use_tree("use a::{b::{C, D}, e::F};");
            let mut paths = resolve_use_tree(&tree, "");
            paths.sort();
            assert_eq!(paths, vec!["a::b::C", "a::b::D", "a::e::F"]);
        }

        #[test]
        fn test_empty_prefix_root_level() {
            let tree = parse_use_tree("use std;");
            let paths = resolve_use_tree(&tree, "");
            assert_eq!(paths, vec!["std"]);
        }

        #[test]
        fn test_with_prefix() {
            let tree = parse_use_tree("use bar::Baz;");
            let paths = resolve_use_tree(&tree, "foo");
            assert_eq!(paths, vec!["foo::bar::Baz"]);
        }
    }

    mod entry_point_tests {
        use super::*;

        #[test]
        fn test_entry_point_export_detected() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]);
            let exports: HashMap<String, HashSet<String>> = HashMap::from([(
                "other_crate".to_string(),
                HashSet::from(["MyStruct".into()]),
            )]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::MyStruct;");
            let deps = parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &exports);
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_crate, "other_crate");
            assert_eq!(dep.target_module, "");
            assert_eq!(dep.target_item, Some("MyStruct".to_string()));
        }

        #[test]
        fn test_non_export_stays_fallback() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]);
            let exports: HashMap<String, HashSet<String>> =
                HashMap::from([("other_crate".to_string(), HashSet::new())]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::Unknown;");
            let deps = parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &exports);
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_module, "Unknown");
            assert!(dep.target_item.is_none());
        }

        #[test]
        fn test_real_module_not_affected() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> =
                HashMap::from([("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]);
            let exports: HashMap<String, HashSet<String>> =
                HashMap::from([("other_crate".to_string(), HashSet::from(["sub_mod".into()]))]);
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::sub_mod::Foo;");
            let deps = parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &exports);
            assert_eq!(deps.len(), 1);
            let dep = &deps[0];
            assert_eq!(dep.target_module, "sub_mod");
            assert_eq!(dep.target_item, Some("Foo".to_string()));
        }

        #[test]
        fn test_entry_point_multi_import() {
            // Without module path info, `use other_crate::{Foo, Bar}` resolves each
            // symbol as a module-level fallback (target_module="Foo", no target_item).
            // The dependency on other_crate is still correctly detected.
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::{Foo, Bar};");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 2, "should return 2 deps: {:?}", deps);
            assert!(deps.iter().all(|d| d.target_crate == "other_crate"));
            assert!(deps.iter().any(|d| d.target_module == "Foo"));
            assert!(deps.iter().any(|d| d.target_module == "Bar"));
        }

        #[test]
        fn test_entry_point_glob_import() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let path = Path::new("src/lib.rs");
            let uses = parse_test_uses("use other_crate::*;");
            let deps =
                parse_workspace_dependencies(&uses, "my_crate", &ws, path, &mp, &HashMap::new());
            assert_eq!(deps.len(), 1, "glob should return 1 dep: {:?}", deps);
            assert_eq!(deps[0].target_module, "");
            assert_eq!(deps[0].target_item, Some("*".to_string()));
        }
    }

    mod collect_use_items_tests {
        use super::*;

        #[test]
        fn top_level_use_found() {
            let syntax = syn::parse_file("use foo::Bar;").unwrap();
            let uses = collect_all_use_items(&syntax);
            assert_eq!(uses.len(), 1);
        }

        #[test]
        fn use_in_fn_body_found() {
            let source = r#"
fn main() {
    use foo::Bar;
}
"#;
            let syntax = syn::parse_file(source).unwrap();
            let uses = collect_all_use_items(&syntax);
            assert_eq!(uses.len(), 1, "use inside fn body must be found");
        }

        #[test]
        fn use_in_nested_block_found() {
            let source = r#"
fn main() {
    {
        use foo::Bar;
    }
}
"#;
            let syntax = syn::parse_file(source).unwrap();
            let uses = collect_all_use_items(&syntax);
            assert_eq!(uses.len(), 1, "use in nested block must be found");
        }

        #[test]
        fn mixed_top_level_and_fn_body() {
            let source = r#"
use crate::config;

fn main() {
    use other_crate::utils;
}
"#;
            let syntax = syn::parse_file(source).unwrap();
            let uses = collect_all_use_items(&syntax);
            assert_eq!(
                uses.len(),
                2,
                "both top-level and fn-body uses must be found"
            );
        }

        #[test]
        fn use_in_cfg_block_inside_fn() {
            let source = r#"
fn main() {
    #[cfg(feature = "a")]
    {
        use my_lib::config::Config;
        use my_lib::engine::Engine;
    }
}
"#;
            let syntax = syn::parse_file(source).unwrap();
            let uses = collect_all_use_items(&syntax);
            assert_eq!(
                uses.len(),
                2,
                "uses in cfg-gated block inside fn must be found"
            );
        }
    }
}
