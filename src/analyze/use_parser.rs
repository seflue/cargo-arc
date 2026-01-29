//! Text-based use statement parsing for workspace dependency extraction.

use crate::model::DependencyRef;
use std::collections::{HashMap, HashSet};
use std::path::Path;

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

/// Extract the use path from a line like `use crate::module;` → `crate::module`
fn extract_use_path(line: &str) -> Option<&str> {
    let line = line.trim();
    if !line.starts_with("use ") {
        return None;
    }
    Some(line.strip_prefix("use ")?.trim_end_matches(';').trim())
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

/// Parse workspace crate imports: `use other_crate::module[::item]`
fn parse_workspace_import(
    path: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    line_num: usize,
    all_module_paths: &HashMap<String, HashSet<String>>,
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

    Some(DependencyRef {
        target_crate: crate_name.to_string(),
        target_module,
        target_item: extract_item_from_parts(&parts, 1 + prefix_len),
        source_file: source_file.to_path_buf(),
        line: line_num,
    })
}

/// Process a single use statement line, returning a DependencyRef if it's a relevant import.
///
/// Handles:
/// - `use crate::module;` - crate-local imports
/// - `use crate::module::item;` - crate-local item imports
/// - `use workspace_crate::module;` - workspace crate imports (when in workspace_crates set)
///
/// Returns None for:
/// - `use self::*` or `use super::*` - relative imports
/// - External crate imports (not in workspace_crates)
fn process_use_statement(
    line: &str,
    line_num: usize,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    all_module_paths: &HashMap<String, HashSet<String>>,
) -> Option<DependencyRef> {
    let path = extract_use_path(line)?;

    parse_crate_local_import(path, current_crate, source_file, line_num, all_module_paths).or_else(
        || {
            parse_workspace_import(
                path,
                workspace_crates,
                source_file,
                line_num,
                all_module_paths,
            )
        },
    )
}

/// Process a use statement that may contain multiple symbols (`{A, B, C}`) or glob (`*`).
/// Returns a Vec of DependencyRefs, one per symbol.
///
/// Handles:
/// - `use crate::module::{A, B, C}` → 3 DependencyRefs
/// - `use crate::module::*` → 1 DependencyRef with target_item = "*"
/// - `use crate::module::Item` → 1 DependencyRef (simple import)
fn process_use_statement_multi(
    line: &str,
    line_num: usize,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    all_module_paths: &HashMap<String, HashSet<String>>,
) -> Vec<DependencyRef> {
    let path = match extract_use_path(line) {
        Some(p) => p,
        None => return vec![],
    };

    // Check for multi-symbol import: `use path::{A, B, C}`
    if let Some(brace_start) = path.find('{')
        && let Some(brace_end) = path.find('}')
    {
        let base_path = path[..brace_start].trim_end_matches(':');
        let symbols_str = &path[brace_start + 1..brace_end];
        let symbols: Vec<&str> = symbols_str
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        // Parse base path to get crate and module
        if let Some((target_crate, target_module)) =
            parse_base_path(base_path, current_crate, workspace_crates, all_module_paths)
        {
            return symbols
                .into_iter()
                .map(|sym| DependencyRef {
                    target_crate: target_crate.clone(),
                    target_module: target_module.clone(),
                    target_item: Some(sym.to_string()),
                    source_file: source_file.to_path_buf(),
                    line: line_num,
                })
                .collect();
        }
        return vec![];
    }

    // Check for glob import: `use path::*`
    if path.ends_with("::*") {
        let base_path = path.trim_end_matches("::*");
        if let Some((target_crate, target_module)) =
            parse_base_path(base_path, current_crate, workspace_crates, all_module_paths)
        {
            return vec![DependencyRef {
                target_crate,
                target_module,
                target_item: Some("*".to_string()),
                source_file: source_file.to_path_buf(),
                line: line_num,
            }];
        }
        return vec![];
    }

    // Fall back to simple import
    if let Some(dep) = process_use_statement(
        line,
        line_num,
        current_crate,
        workspace_crates,
        source_file,
        all_module_paths,
    ) {
        return vec![dep];
    }

    vec![]
}

/// Parse a base path (before `::*` or `::{...}`) into (crate, module).
fn parse_base_path(
    base_path: &str,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    all_module_paths: &HashMap<String, HashSet<String>>,
) -> Option<(String, String)> {
    let empty_set = HashSet::new();

    // Handle crate-local: `crate::module`
    if let Some(after_crate) = base_path.strip_prefix("crate::") {
        let parts: Vec<&str> = after_crate.split("::").collect();
        if parts.is_empty() || parts[0].is_empty() {
            return None;
        }
        let module_paths = all_module_paths
            .get(&normalize_crate_name(current_crate))
            .unwrap_or(&empty_set);
        let (module, _prefix_len) = find_longest_module_prefix(&parts, module_paths);
        return Some((normalize_crate_name(current_crate), module));
    }

    // Handle workspace crate: `other_crate::module`
    let parts: Vec<&str> = base_path.split("::").collect();
    if parts.len() >= 2 {
        let first_segment = parts[0].trim();
        let is_workspace_crate = is_workspace_member(first_segment, workspace_crates);

        if is_workspace_crate {
            let target_crate = normalize_crate_name(first_segment);
            let module_paths = all_module_paths.get(&target_crate).unwrap_or(&empty_set);
            let (module, _prefix_len) = find_longest_module_prefix(&parts[1..], module_paths);
            return Some((first_segment.to_string(), module));
        }
    }

    None
}

/// Parse use statements from source code, extracting workspace-relevant dependencies.
///
/// Returns DependencyRefs for:
/// - Crate-local imports (`use crate::module`)
/// - Workspace crate imports (`use other_crate::module` where other_crate is in workspace)
///
/// Deduplicates by full_target() to keep distinct symbols but avoid duplicates.
pub(crate) fn parse_workspace_dependencies(
    source: &str,
    current_crate: &str,
    workspace_crates: &HashSet<String>,
    source_file: &Path,
    all_module_paths: &HashMap<String, HashSet<String>>,
) -> Vec<DependencyRef> {
    let mut deps: Vec<DependencyRef> = Vec::new();
    let mut seen_targets: HashSet<String> = HashSet::new();

    for (line_idx, line) in source.lines().enumerate() {
        let line_num = line_idx + 1;
        for dep in process_use_statement_multi(
            line,
            line_num,
            current_crate,
            workspace_crates,
            source_file,
            all_module_paths,
        ) {
            let target_key = dep.full_target();
            if !seen_targets.contains(&target_key) {
                seen_targets.insert(target_key);
                deps.push(dep);
            }
        }
    }

    deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
            let dep = process_use_statement(
                "use crate::graph::build;",
                1,
                "my_crate",
                &ws,
                Path::new("src/cli.rs"),
                &mp,
            );
            let dep = dep.expect("should parse crate-local import");
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "graph");
            assert_eq!(dep.target_item, Some("build".to_string()));
        }

        #[test]
        fn test_process_use_statement_crate_local_module_only() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use crate::graph;",
                5,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            let dep = dep.expect("should parse crate-local module import");
            assert_eq!(dep.target_crate, "my_crate");
            assert_eq!(dep.target_module, "graph");
            assert!(dep.target_item.is_none());
            assert_eq!(dep.line, 5);
        }

        #[test]
        fn test_process_use_statement_workspace_crate() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use other_crate::utils;",
                1,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            let dep = dep.expect("should parse workspace crate import");
            assert_eq!(dep.target_crate, "other_crate");
            assert_eq!(dep.target_module, "utils");
        }

        #[test]
        fn test_process_use_statement_workspace_crate_with_hyphen() {
            let ws: HashSet<String> = HashSet::from(["my-lib".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use my_lib::feature;",
                1,
                "app",
                &ws,
                Path::new("src/main.rs"),
                &mp,
            );
            let dep = dep.expect("should parse workspace crate with hyphen");
            assert_eq!(dep.target_crate, "my_lib");
            assert_eq!(dep.target_module, "feature");
        }

        #[test]
        fn test_process_use_statement_relative_self_ignored() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use self::helper;",
                1,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            assert!(dep.is_none(), "self:: imports should be ignored");
        }

        #[test]
        fn test_process_use_statement_relative_super_ignored() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use super::parent;",
                1,
                "my_crate",
                &ws,
                Path::new("src/sub/mod.rs"),
                &mp,
            );
            assert!(dep.is_none(), "super:: imports should be ignored");
        }

        #[test]
        fn test_process_use_statement_external_filtered() {
            let ws: HashSet<String> = HashSet::from(["my_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use serde::Serialize;",
                1,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            assert!(dep.is_none(), "external crate imports should be filtered");
        }

        #[test]
        fn test_process_use_statement_std_filtered() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use std::collections::HashMap;",
                1,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            assert!(dep.is_none(), "std imports should be filtered");
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
            let dep = process_use_statement(
                "use crate::analyze::use_parser::normalize;",
                1,
                "my_crate",
                &ws,
                Path::new("src/cli.rs"),
                &mp,
            );
            let dep = dep.expect("should parse crate-local submodule import");
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
            let dep = process_use_statement(
                "use other_crate::foo::bar::Baz;",
                1,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            let dep = dep.expect("should parse workspace submodule import");
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
            let dep = process_use_statement(
                "use other_crate::sub::deep::Item;",
                1,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            let dep = dep.expect("should parse deep cross-crate import");
            assert_eq!(dep.target_module, "sub::deep");
            assert_eq!(dep.target_item, Some("Item".to_string()));
        }

        #[test]
        fn test_cross_crate_no_paths_fallback() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let dep = process_use_statement(
                "use other_crate::foo::bar::Baz;",
                1,
                "my_crate",
                &ws,
                Path::new("src/lib.rs"),
                &mp,
            );
            let dep = dep.expect("should parse with fallback");
            assert_eq!(dep.target_module, "foo");
            assert_eq!(dep.target_item, Some("bar".to_string()));
        }

        #[test]
        fn test_parse_base_path_with_submodule() {
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "my_crate".to_string(),
                HashSet::from(["analyze::use_parser".into()]),
            )]);
            let ws: HashSet<String> = HashSet::new();
            let result = parse_base_path("crate::analyze::use_parser", "my_crate", &ws, &mp);
            assert_eq!(
                result,
                Some(("my_crate".to_string(), "analyze::use_parser".to_string()))
            );
        }

        #[test]
        fn test_parse_base_path_workspace_submodule() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "other_crate".to_string(),
                HashSet::from(["foo::bar".into()]),
            )]);
            let result = parse_base_path("other_crate::foo::bar", "my_crate", &ws, &mp);
            assert_eq!(
                result,
                Some(("other_crate".to_string(), "foo::bar".to_string()))
            );
        }

        #[test]
        fn test_parse_base_path_cross_crate_no_paths() {
            let ws: HashSet<String> = HashSet::from(["other_crate".to_string()]);
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let result = parse_base_path("other_crate::foo::bar", "my_crate", &ws, &mp);
            assert_eq!(result, Some(("other_crate".to_string(), "foo".to_string())));
        }

        #[test]
        fn test_parse_base_path_glob_stays_parent() {
            let mp: HashMap<String, HashSet<String>> = HashMap::new();
            let ws: HashSet<String> = HashSet::new();
            let result = parse_base_path("crate::analyze", "my_crate", &ws, &mp);
            assert_eq!(
                result,
                Some(("my_crate".to_string(), "analyze".to_string()))
            );
        }

        #[test]
        fn test_multi_symbol_with_submodule() {
            let ws: HashSet<String> = HashSet::new();
            let mp: HashMap<String, HashSet<String>> = HashMap::from([(
                "my_crate".to_string(),
                HashSet::from(["analyze".into(), "analyze::use_parser".into()]),
            )]);
            let deps = process_use_statement_multi(
                "use crate::analyze::use_parser::{normalize, is_workspace_member};",
                1,
                "my_crate",
                &ws,
                Path::new("src/cli.rs"),
                &mp,
            );
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
            let deps =
                parse_workspace_dependencies(source, "my_crate", &ws, Path::new("src/lib.rs"), &mp);

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
            let deps =
                parse_workspace_dependencies(source, "my_crate", &ws, Path::new("src/cli.rs"), &mp);

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
            let deps = process_use_statement_multi(
                "use crate::graph::{Node, Edge};",
                1,
                "my_crate",
                &ws,
                Path::new("src/cli.rs"),
                &mp,
            );
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
            let deps = process_use_statement_multi(
                "use crate::analyze::*;",
                1,
                "my_crate",
                &ws,
                Path::new("src/cli.rs"),
                &mp,
            );
            assert_eq!(deps.len(), 1, "glob should return 1 dep: {:?}", deps);
            assert_eq!(deps[0].target_item, Some("*".to_string()));
            assert_eq!(deps[0].target_module, "analyze");
        }
    }
}
