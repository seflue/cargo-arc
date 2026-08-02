use std::fmt::Write as _;

/// Parsed JS module metadata from @module/@deps/@config annotations.
#[derive(Debug, Clone)]
struct JsModuleInfo {
    name: String,
    file_name: String,
    deps: Vec<String>,
    config_keys: Vec<String>,
}

/// Check if a filename is a module file (not test, not hidden).
fn is_module_file(file_name: &str) -> bool {
    std::path::Path::new(file_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
        && !file_name.ends_with(".test.js")
}

/// Parse @module, @deps, @config from first 5 lines of JS content.
///
/// Panics if @module annotation is missing.
fn parse_js_annotations(content: &str, file_name: &str) -> JsModuleInfo {
    let mut name = None;
    let mut deps = Vec::new();
    let mut config_keys = Vec::new();

    for line in content.lines().take(5) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("// @module") {
            let val = rest.trim();
            if !val.is_empty() {
                name = Some(val.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("// @deps") {
            let val = rest.trim();
            if !val.is_empty() {
                deps = val.split(',').map(|s| s.trim().to_string()).collect();
            }
        } else if let Some(rest) = line.strip_prefix("// @config") {
            let val = rest.trim();
            if !val.is_empty() {
                config_keys = val.split(',').map(|s| s.trim().to_string()).collect();
            }
        }
    }

    JsModuleInfo {
        name: name.unwrap_or_else(|| panic!("missing @module annotation in {file_name}")),
        file_name: file_name.to_string(),
        deps,
        config_keys,
    }
}

/// Topological sort using Kahn's algorithm.
///
/// Returns indices into the input slice, in dependency order.
/// Tie-breaking: alphabetical by module name.
/// Panics on cycle detected or unknown dependency.
fn topo_sort(modules: &[JsModuleInfo]) -> Vec<usize> {
    use std::collections::{BTreeSet, HashMap};

    let name_to_idx: HashMap<&str, usize> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name.as_str(), i))
        .collect();

    // Validate all deps exist
    for m in modules {
        for dep in &m.deps {
            assert!(
                name_to_idx.contains_key(dep.as_str()),
                "unknown dependency '{}' in module '{}'",
                dep,
                m.name
            );
        }
    }

    // Build in-degree counts
    let mut in_degree = vec![0usize; modules.len()];
    for (i, m) in modules.iter().enumerate() {
        in_degree[i] = m.deps.len();
    }

    // Ready set: modules with in-degree 0, sorted alphabetically by name
    let mut ready: BTreeSet<(String, usize)> = BTreeSet::new();
    for (i, m) in modules.iter().enumerate() {
        if in_degree[i] == 0 {
            ready.insert((m.name.clone(), i));
        }
    }

    // Build reverse adjacency: dep_idx → vec of dependent indices
    let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, m) in modules.iter().enumerate() {
        for dep in &m.deps {
            let dep_idx = name_to_idx[dep.as_str()];
            dependents.entry(dep_idx).or_default().push(i);
        }
    }

    let mut result = Vec::with_capacity(modules.len());
    while let Some((_, idx)) = ready.iter().next().cloned() {
        ready.remove(&(modules[idx].name.clone(), idx));
        result.push(idx);

        if let Some(deps) = dependents.get(&idx) {
            for &dep_idx in deps {
                in_degree[dep_idx] -= 1;
                if in_degree[dep_idx] == 0 {
                    ready.insert((modules[dep_idx].name.clone(), dep_idx));
                }
            }
        }
    }

    if result.len() != modules.len() {
        let remaining: Vec<&str> = modules
            .iter()
            .enumerate()
            .filter(|(i, _)| !result.contains(i))
            .map(|(_, m)| m.name.as_str())
            .collect();
        panic!("cycle detected among modules: {remaining:?}");
    }

    result
}

/// Extract lines that are not comments and not in the `CommonJS` export block.
///
/// Skips:
/// - Lines starting with `//` or `*` (comments, `JSDoc`)
/// - Everything from `if (typeof module` to end of file (`CommonJS` export)
fn active_source_lines(source: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("if (typeof module") {
            break;
        }
        if trimmed.starts_with("//") || trimmed.starts_with('*') {
            continue;
        }
        lines.push(trimmed);
    }
    lines
}

/// Validate that all module references in source code are declared in @deps.
///
/// For each module, scans active lines for `OtherModule.` patterns.
/// Panics if a reference is found but not in @deps.
fn validate_source_deps(modules: &[JsModuleInfo], sources: &[&str]) {
    use std::collections::HashSet;

    let all_names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();

    for (i, module) in modules.iter().enumerate() {
        let declared: HashSet<&str> = module
            .deps
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let lines = active_source_lines(sources[i]);

        for name in &all_names {
            if *name == module.name.as_str() {
                continue;
            }
            if declared.contains(name) {
                continue;
            }
            let pattern = format!("{name}.");
            for line in &lines {
                assert!(
                    !line.contains(&pattern),
                    "module '{}' ({}) references {}.* but '{}' is not in @deps",
                    module.name,
                    module.file_name,
                    name,
                    name
                );
            }
        }
    }
}

/// Generate Rust source code for `js_modules.rs`.
///
/// Output: struct `JsModule` + const MODULES array with `include_str`!().
fn generate_modules_rs(modules: &[JsModuleInfo], sorted_indices: &[usize]) -> String {
    let mut out = String::new();
    out.push_str("#[allow(dead_code)]\n");
    out.push_str("struct JsModule {\n");
    out.push_str("    name: &'static str,\n");
    out.push_str("    source: &'static str,\n");
    out.push_str("    config_keys: &'static [&'static str],\n");
    out.push_str("}\n\n");

    out.push_str("const MODULES: &[JsModule] = &[\n");
    for &idx in sorted_indices {
        let m = &modules[idx];
        out.push_str("    JsModule {\n");
        let _ = writeln!(out, "        name: \"{}\",", m.name);
        let _ = writeln!(
            out,
            "        source: include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/js/{}\")),",
            m.file_name
        );
        if m.config_keys.is_empty() {
            out.push_str("        config_keys: &[],\n");
        } else {
            let keys: Vec<String> = m.config_keys.iter().map(|k| format!("\"{k}\"")).collect();
            let _ = writeln!(out, "        config_keys: &[{}],", keys.join(", "));
        }
        out.push_str("    },\n");
    }
    out.push_str("];\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cargo-arc JS module set (name, file, deps, config keys), fixed to catch drift.
    fn real_modules() -> Vec<JsModuleInfo> {
        const TABLE: &[(&str, &str, &[&str], &[&str])] = &[
            ("ArcLogic", "arc_logic.js", &[], &[]),
            ("TreeLogic", "tree_logic.js", &[], &[]),
            ("AppState", "app_state.js", &[], &[]),
            ("Selectors", "selectors.js", &[], &[]),
            ("TextMetrics", "text_metrics.js", &[], &[]),
            ("LayerManager", "layer_manager.js", &[], &[]),
            ("StaticData", "static_data.js", &["ArcLogic"], &[]),
            ("DomAdapter", "dom_adapter.js", &["Selectors"], &[]),
            ("HighlightLogic", "highlight_logic.js", &["ArcLogic"], &[]),
            (
                "DerivedState",
                "derived_state.js",
                &["TreeLogic", "ArcLogic", "AppState", "HighlightLogic"],
                &[],
            ),
            (
                "VirtualEdgeLogic",
                "virtual_edge_logic.js",
                &["ArcLogic"],
                &[],
            ),
            (
                "SidebarLogic",
                "sidebar.js",
                &["StaticData", "DomAdapter", "Selectors"],
                &[],
            ),
            (
                "HighlightRenderer",
                "highlight_renderer.js",
                &["ArcLogic", "LayerManager"],
                &[],
            ),
            (
                "SvgScript",
                "svg_script.js",
                &[
                    "ArcLogic",
                    "StaticData",
                    "AppState",
                    "Selectors",
                    "DomAdapter",
                    "LayerManager",
                    "TreeLogic",
                    "DerivedState",
                    "HighlightRenderer",
                    "VirtualEdgeLogic",
                    "TextMetrics",
                    "SidebarLogic",
                ],
                &["ROW_HEIGHT", "MARGIN", "TOOLBAR_HEIGHT"],
            ),
        ];

        TABLE
            .iter()
            .map(|&(name, file_name, deps, config_keys)| JsModuleInfo {
                name: name.into(),
                file_name: file_name.into(),
                deps: deps.iter().map(std::string::ToString::to_string).collect(),
                config_keys: config_keys
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect(),
            })
            .collect()
    }

    // --- Parsing tests ---

    #[test]
    fn test_parse_annotations_full() {
        let content = "\
// @module SvgScript
// @deps ArcLogic, StaticData, AppState
// @config ROW_HEIGHT, MARGIN, TOOLBAR_HEIGHT
// svg_script.js
";
        let info = parse_js_annotations(content, "svg_script.js");
        assert_eq!(info.name, "SvgScript");
        assert_eq!(info.file_name, "svg_script.js");
        assert_eq!(info.deps, vec!["ArcLogic", "StaticData", "AppState"]);
        assert_eq!(
            info.config_keys,
            vec!["ROW_HEIGHT", "MARGIN", "TOOLBAR_HEIGHT"]
        );
    }

    #[test]
    fn test_parse_annotations_no_deps() {
        let content = "\
// @module ArcLogic
// @deps
// @config
// arc_logic.js
";
        let info = parse_js_annotations(content, "arc_logic.js");
        assert_eq!(info.name, "ArcLogic");
        assert!(info.deps.is_empty());
        assert!(info.config_keys.is_empty());
    }

    #[test]
    fn test_parse_annotations_single_dep() {
        let content = "\
// @module StaticData
// @deps ArcLogic
// @config
// static_data.js
";
        let info = parse_js_annotations(content, "static_data.js");
        assert_eq!(info.deps, vec!["ArcLogic"]);
    }

    #[test]
    fn test_is_module_file() {
        assert!(is_module_file("foo.js"));
        assert!(is_module_file("arc_logic.js"));
        assert!(!is_module_file("foo.test.js"));
        assert!(!is_module_file("foo.rs"));
        assert!(!is_module_file("foo.ts"));
    }

    #[test]
    #[should_panic(expected = "missing @module")]
    fn test_missing_module_panics() {
        let content = "// just a comment\nfunction foo() {}\n";
        parse_js_annotations(content, "no_module.js");
    }

    // --- Topo sort tests ---

    #[test]
    fn test_topo_sort_simple() {
        let modules = vec![
            JsModuleInfo {
                name: "C".into(),
                file_name: "c.js".into(),
                deps: vec!["B".into()],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "B".into(),
                file_name: "b.js".into(),
                deps: vec!["A".into()],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "A".into(),
                file_name: "a.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
        ];
        let sorted = topo_sort(&modules);
        let names: Vec<&str> = sorted.iter().map(|&i| modules[i].name.as_str()).collect();
        assert_eq!(names, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_topo_sort_multiple_roots() {
        let modules = vec![
            JsModuleInfo {
                name: "Selectors".into(),
                file_name: "selectors.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "ArcLogic".into(),
                file_name: "arc_logic.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "AppState".into(),
                file_name: "app_state.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
        ];
        let sorted = topo_sort(&modules);
        let names: Vec<&str> = sorted.iter().map(|&i| modules[i].name.as_str()).collect();
        // Alphabetical: AppState < ArcLogic < Selectors
        assert_eq!(names, vec!["AppState", "ArcLogic", "Selectors"]);
    }

    #[test]
    fn test_topo_sort_diamond() {
        let modules = vec![
            JsModuleInfo {
                name: "C".into(),
                file_name: "c.js".into(),
                deps: vec!["A".into(), "B".into()],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "B".into(),
                file_name: "b.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "A".into(),
                file_name: "a.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
        ];
        let sorted = topo_sort(&modules);
        let names: Vec<&str> = sorted.iter().map(|&i| modules[i].name.as_str()).collect();
        assert_eq!(names, vec!["A", "B", "C"]);
    }

    #[test]
    #[should_panic(expected = "cycle")]
    fn test_topo_sort_cycle_panics() {
        let modules = vec![
            JsModuleInfo {
                name: "A".into(),
                file_name: "a.js".into(),
                deps: vec!["B".into()],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "B".into(),
                file_name: "b.js".into(),
                deps: vec!["A".into()],
                config_keys: vec![],
            },
        ];
        topo_sort(&modules);
    }

    #[test]
    #[should_panic(expected = "unknown")]
    fn test_topo_sort_missing_dep_panics() {
        let modules = vec![JsModuleInfo {
            name: "A".into(),
            file_name: "a.js".into(),
            deps: vec!["X".into()],
            config_keys: vec![],
        }];
        topo_sort(&modules);
    }

    #[test]
    fn test_topo_sort_real_modules() {
        let modules = real_modules();

        let sorted = topo_sort(&modules);
        let names: Vec<&str> = sorted.iter().map(|&i| modules[i].name.as_str()).collect();

        // SvgScript must be last
        assert_eq!(*names.last().unwrap(), "SvgScript");

        // Every dep must appear before its dependent
        for (pos, &idx) in sorted.iter().enumerate() {
            for dep in &modules[idx].deps {
                let dep_pos = names.iter().position(|&n| n == dep).unwrap();
                assert!(
                    dep_pos < pos,
                    "{} (pos {}) must come after {} (pos {})",
                    modules[idx].name,
                    pos,
                    dep,
                    dep_pos
                );
            }
        }
    }

    // --- Code generation tests ---

    #[test]
    fn test_generate_modules_rs_format() {
        let modules = vec![
            JsModuleInfo {
                name: "A".into(),
                file_name: "a.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "B".into(),
                file_name: "b.js".into(),
                deps: vec!["A".into()],
                config_keys: vec!["KEY1".into()],
            },
        ];
        let sorted = topo_sort(&modules);
        let output = generate_modules_rs(&modules, &sorted);

        assert!(output.contains("struct JsModule"));
        assert!(output.contains("const MODULES: &[JsModule]"));
        // Two entries
        assert_eq!(output.matches("JsModule {").count(), 3); // 1 struct def + 2 entries
    }

    #[test]
    fn test_generate_modules_rs_order() {
        let modules = vec![
            JsModuleInfo {
                name: "B".into(),
                file_name: "b.js".into(),
                deps: vec!["A".into()],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "A".into(),
                file_name: "a.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
        ];
        let sorted = topo_sort(&modules);
        let output = generate_modules_rs(&modules, &sorted);

        let pos_a = output.find("name: \"A\"").unwrap();
        let pos_b = output.find("name: \"B\"").unwrap();
        assert!(pos_a < pos_b, "A must appear before B in output");
    }

    #[test]
    fn test_generate_modules_rs_config_keys() {
        let modules = vec![JsModuleInfo {
            name: "M".into(),
            file_name: "m.js".into(),
            deps: vec![],
            config_keys: vec!["ROW_HEIGHT".into(), "MARGIN".into()],
        }];
        let sorted = vec![0];
        let output = generate_modules_rs(&modules, &sorted);

        assert!(output.contains("config_keys: &[\"ROW_HEIGHT\", \"MARGIN\"]"));
    }

    // --- Source scan validation tests ---

    #[test]
    #[should_panic(expected = "references ArcLogic")]
    fn test_source_scan_finds_undeclared_dep() {
        let modules = vec![
            JsModuleInfo {
                name: "ArcLogic".into(),
                file_name: "arc_logic.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "Foo".into(),
                file_name: "foo.js".into(),
                deps: vec![], // ArcLogic NOT declared
                config_keys: vec![],
            },
        ];
        let sources = &["const ArcLogic = {};", "var x = ArcLogic.calc();"];
        validate_source_deps(&modules, sources);
    }

    #[test]
    fn test_source_scan_ignores_comments() {
        let modules = vec![
            JsModuleInfo {
                name: "ArcLogic".into(),
                file_name: "arc_logic.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "Foo".into(),
                file_name: "foo.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
        ];
        let sources = &[
            "const ArcLogic = {};",
            "// ArcLogic.calc is loaded before\n * ArcLogic.stuff in JSDoc\nvar x = 1;",
        ];
        validate_source_deps(&modules, sources); // no panic
    }

    #[test]
    fn test_source_scan_ignores_commonjs() {
        let modules = vec![
            JsModuleInfo {
                name: "ArcLogic".into(),
                file_name: "arc_logic.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
            JsModuleInfo {
                name: "Foo".into(),
                file_name: "foo.js".into(),
                deps: vec![],
                config_keys: vec![],
            },
        ];
        let sources = &[
            "const ArcLogic = {};",
            "var x = 1;\nif (typeof module !== 'undefined') {\n  module.exports = { ArcLogic };\n}",
        ];
        validate_source_deps(&modules, sources); // no panic
    }

    #[test]
    fn test_source_scan_self_reference_ok() {
        let modules = vec![JsModuleInfo {
            name: "ArcLogic".into(),
            file_name: "arc_logic.js".into(),
            deps: vec![],
            config_keys: vec![],
        }];
        let sources = &["const ArcLogic = {};\nArcLogic.calc = function() {};"];
        validate_source_deps(&modules, sources); // no panic
    }

    #[test]
    fn test_source_scan_real_modules() {
        let modules = real_modules();
        let sources: Vec<&str> = vec![
            include_str!("../js/arc_logic.js"),
            include_str!("../js/tree_logic.js"),
            include_str!("../js/app_state.js"),
            include_str!("../js/selectors.js"),
            include_str!("../js/text_metrics.js"),
            include_str!("../js/layer_manager.js"),
            include_str!("../js/static_data.js"),
            include_str!("../js/dom_adapter.js"),
            include_str!("../js/highlight_logic.js"),
            include_str!("../js/derived_state.js"),
            include_str!("../js/virtual_edge_logic.js"),
            include_str!("../js/sidebar.js"),
            include_str!("../js/highlight_renderer.js"),
            include_str!("../js/svg_script.js"),
        ];
        validate_source_deps(&modules, &sources); // no panic — all deps correctly declared
    }
}
