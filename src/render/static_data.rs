use super::constants::{CSS, LAYOUT, RenderConfig};
use super::positioning::PositionedItem;
use crate::layout::{ItemKind, LayoutIR, NodeId};
use crate::model::SourceLocation;
use std::collections::HashSet;

include!(concat!(env!("OUT_DIR"), "/js_modules.rs"));

/// Format source locations grouped by symbol.
///
/// Inverts the Location→Symbols structure to Symbol→Locations for structured display.
///
/// Returns a Vec of SymbolUsageGroup objects. Bare locations (without symbols)
/// are returned with symbol="". Groups are ordered: bare locations first, then
/// symbol groups alphabetically.
fn format_source_locations_by_symbol(locs: &[SourceLocation]) -> Vec<SymbolUsageGroup> {
    use std::collections::BTreeMap;

    if locs.is_empty() {
        return Vec::new();
    }

    let module_path = locs
        .first()
        .map(|l| l.module_path.clone())
        .unwrap_or_default();

    // Invert: Symbol → Vec<(file, line)>
    let mut by_symbol: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    let mut bare_locations: Vec<(String, usize)> = Vec::new();

    for loc in locs {
        let file_str = loc.file.display().to_string();
        if loc.symbols.is_empty() {
            // Location without symbols - collect separately
            bare_locations.push((file_str, loc.line));
        } else {
            for symbol in &loc.symbols {
                by_symbol
                    .entry(symbol.clone())
                    .or_default()
                    .push((file_str.clone(), loc.line));
            }
        }
    }

    // Sort locations within each symbol alphabetically
    for locations in by_symbol.values_mut() {
        locations.sort();
    }

    let mut groups = Vec::new();

    // First: bare locations (symbol = "")
    if !bare_locations.is_empty() {
        bare_locations.sort();
        groups.push(SymbolUsageGroup {
            symbol: String::new(),
            module_path: module_path.clone(),
            locations: bare_locations
                .into_iter()
                .map(|(file, line)| UsageLocation { file, line })
                .collect(),
        });
    }

    // Then: symbol-grouped locations in alphabetical order
    for (symbol, locations) in by_symbol {
        groups.push(SymbolUsageGroup {
            symbol,
            module_path: module_path.clone(),
            locations: locations
                .into_iter()
                .map(|(file, line)| UsageLocation { file, line })
                .collect(),
        });
    }

    groups
}

/// A group of usage locations for a single symbol
struct SymbolUsageGroup {
    symbol: String,
    module_path: String,
    locations: Vec<UsageLocation>,
}

/// A single usage location (file + line number)
struct UsageLocation {
    file: String,
    line: usize,
}

fn generate_nodes_js(
    ir: &LayoutIR,
    positioned: &[PositionedItem],
    parents: &HashSet<NodeId>,
) -> String {
    let mut lines = Vec::new();
    lines.push("  nodes: {".to_string());
    for (i, pos) in positioned.iter().enumerate() {
        let item = &ir.items[pos.id];
        let node_type = match &item.kind {
            ItemKind::Crate => "crate",
            ItemKind::Module { .. } => "module",
        };
        let parent_str = match &item.kind {
            ItemKind::Crate => "null".to_string(),
            ItemKind::Module { parent, .. } => format!("\"{}\"", parent),
        };
        let has_children = parents.contains(&pos.id);
        let comma = if i < positioned.len() - 1 { "," } else { "" };

        let name_escaped = escape_js_string(&item.label);
        lines.push(format!(
            "    \"{}\": {{ type: \"{}\", name: \"{}\", parent: {}, x: {}, y: {}, width: {}, height: {}, hasChildren: {} }}{}",
            pos.id, node_type, name_escaped, parent_str, pos.x, pos.y, pos.width, pos.height, has_children, comma
        ));
    }
    lines.push("  },".to_string());
    lines.join("\n")
}

fn generate_arcs_js(ir: &LayoutIR) -> String {
    let mut lines = Vec::new();
    lines.push("  arcs: {".to_string());
    for (i, edge) in ir.edges.iter().enumerate() {
        let arc_id = format!("{}-{}", edge.from, edge.to);

        let usages_str = if edge.source_locations.is_empty() {
            "[]".to_string()
        } else {
            let groups = format_source_locations_by_symbol(&edge.source_locations);
            let mut group_strs = Vec::new();
            for group in groups {
                let symbol_escaped = escape_js_string(&group.symbol);
                let mut loc_strs = Vec::new();
                for loc in group.locations {
                    let file_escaped = escape_js_string(&loc.file);
                    loc_strs.push(format!(
                        "{{ file: \"{}\", line: {} }}",
                        file_escaped, loc.line
                    ));
                }
                let mp = escape_js_string(&group.module_path);
                let module_path_js = if mp.is_empty() {
                    "null".to_string()
                } else {
                    format!("\"{}\"", mp)
                };
                group_strs.push(format!(
                    "{{ symbol: \"{}\", modulePath: {}, locations: [{}] }}",
                    symbol_escaped,
                    module_path_js,
                    loc_strs.join(", ")
                ));
            }
            format!("[{}]", group_strs.join(", "))
        };

        let cycle_ids_str = if edge.cycle_ids.is_empty() {
            String::new()
        } else {
            let ids: Vec<String> = edge.cycle_ids.iter().map(|id| id.to_string()).collect();
            format!(", cycleIds: [{}]", ids.join(", "))
        };

        let comma = if i < ir.edges.len() - 1 { "," } else { "" };

        lines.push(format!(
            "    \"{}\": {{ from: \"{}\", to: \"{}\", context: {}, usages: {}{} }}{}",
            arc_id,
            edge.from,
            edge.to,
            edge.context.format_js(),
            usages_str,
            cycle_ids_str,
            comma
        ));
    }
    lines.push("  },".to_string());
    lines.join("\n")
}

fn generate_cycles_js(ir: &LayoutIR) -> String {
    use std::collections::{BTreeMap, BTreeSet};
    let mut cycle_map: BTreeMap<usize, (BTreeSet<NodeId>, BTreeSet<String>)> = BTreeMap::new();
    for edge in &ir.edges {
        for &cid in &edge.cycle_ids {
            let entry = cycle_map.entry(cid).or_default();
            entry.0.insert(edge.from);
            entry.0.insert(edge.to);
            entry.1.insert(format!("{}-{}", edge.from, edge.to));
        }
    }
    let mut lines = Vec::new();
    lines.push("  cycles: [".to_string());
    let cycle_count = cycle_map.len();
    for (i, (_cid, (nodes, arcs))) in cycle_map.iter().enumerate() {
        let nodes_str: Vec<String> = nodes.iter().map(|n| format!("\"{}\"", n)).collect();
        let arcs_str: Vec<String> = arcs.iter().map(|a| format!("\"{}\"", a)).collect();
        let comma = if i < cycle_count - 1 { "," } else { "" };
        lines.push(format!(
            "    {{ nodes: [{}], arcs: [{}] }}{}",
            nodes_str.join(", "),
            arcs_str.join(", "),
            comma
        ));
    }
    lines.push("  ],".to_string());
    lines.join("\n")
}

/// Generate STATIC_DATA JavaScript constant from layout data
fn generate_static_data(
    ir: &LayoutIR,
    positioned: &[PositionedItem],
    parents: &HashSet<NodeId>,
) -> String {
    [
        "const STATIC_DATA = {",
        &generate_nodes_js(ir, positioned, parents),
        &generate_arcs_js(ir),
        &generate_cycles_js(ir),
        &generate_classes_js(),
        "};",
    ]
    .join("\n")
}

fn generate_classes_js() -> String {
    let entries: &[(&str, &str)] = &[
        ("crateNode", CSS.nodes.crate_node),
        ("module", CSS.nodes.module),
        ("label", CSS.nodes.label),
        ("collapseToggle", CSS.nodes.collapse_toggle),
        ("collapsed", CSS.nodes.collapsed),
        ("depArc", CSS.direction.dep_arc),
        ("downward", CSS.direction.downward),
        ("upward", CSS.direction.upward),
        ("depArrow", CSS.direction.dep_arrow),
        ("upwardArrow", CSS.direction.upward_arrow),
        ("cycleArc", CSS.direction.cycle_arc),
        ("cycleArrow", CSS.direction.cycle_arrow),
        ("arcHitarea", CSS.direction.arc_hitarea),
        ("crateDepArc", CSS.direction.crate_dep_arc),
        ("virtualArc", CSS.direction.virtual_arc),
        ("virtualArrow", CSS.direction.virtual_arrow),
        ("virtualHitarea", CSS.direction.virtual_hitarea),
        ("selectedCrate", CSS.node_selection.selected_crate),
        ("selectedModule", CSS.node_selection.selected_module),
        ("groupMember", CSS.node_selection.group_member),
        ("cycleMember", CSS.node_selection.cycle_member),
        ("highlightedArc", CSS.relation.highlighted_arc),
        ("highlightedArrow", CSS.relation.highlighted_arrow),
        ("highlightedLabel", CSS.relation.highlighted_label),
        ("depNode", CSS.relation.dep_node),
        ("dependentNode", CSS.relation.dependent_node),
        ("dimmed", CSS.relation.dimmed),
        ("hasHighlight", CSS.relation.has_highlight),
        ("shadowPath", CSS.relation.shadow_path),
        ("glowIncoming", CSS.relation.glow_incoming),
        ("glowOutgoing", CSS.relation.glow_outgoing),
        ("viewOptions", CSS.toolbar.view_options),
        ("toolbarBtn", CSS.toolbar.btn),
        ("toolbarCheckbox", CSS.toolbar.checkbox),
        ("checked", CSS.toolbar.checked),
        ("arcCount", CSS.labels.arc_count),
        ("arcCountBg", CSS.labels.arc_count_bg),
        ("arcCountGroup", CSS.labels.arc_count_group),
        ("hiddenByFilter", CSS.labels.hidden_by_filter),
    ];
    let mut lines = Vec::new();
    lines.push("  classes: {".to_string());
    let last = entries.len() - 1;
    for (i, (key, value)) in entries.iter().enumerate() {
        let comma = if i < last { "," } else { "" };
        lines.push(format!("    {}: \"{}\"{}", key, value, comma));
    }
    lines.push("  }".to_string());
    lines.join("\n")
}

/// Escape string for JavaScript (handles quotes and backslashes)
fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn render_script(
    config: &RenderConfig,
    ir: &LayoutIR,
    positioned: &[PositionedItem],
    parents: &HashSet<NodeId>,
) -> String {
    // Generate STATIC_DATA first (global scope, before IIFE)
    let static_data = generate_static_data(ir, positioned, parents);

    // JS modules loaded via build.rs-generated registry (topological order)
    let mut scripts = vec![static_data];
    for module in MODULES {
        let mut source = module.source.to_string();
        for key in module.config_keys {
            let placeholder = format!("__{}__", key);
            let value = match *key {
                "ROW_HEIGHT" => config.row_height.to_string(),
                "MARGIN" => config.margin.to_string(),
                "TOOLBAR_HEIGHT" => LAYOUT.toolbar.height.to_string(),
                "SIDEBAR_SHADOW_PAD" => LAYOUT.sidebar.shadow_padding().to_string(),
                other => panic!("Unknown config key: {}", other),
            };
            source = source.replace(&placeholder, &value);
        }
        scripts.push(source);
    }
    format!(
        "  <script><![CDATA[\n{}\n]]></script>\n",
        scripts.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::super::positioning::{calculate_box_width, calculate_positions};
    use super::*;
    use crate::layout::LayoutEdge;
    use crate::model::EdgeContext;

    // === format_source_locations_by_symbol Tests ===

    #[test]
    fn test_format_source_locations_by_symbol_empty() {
        let locs: Vec<SourceLocation> = vec![];
        let groups = format_source_locations_by_symbol(&locs);
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_format_source_locations_by_symbol_no_symbols() {
        use std::path::PathBuf;

        let locs = vec![SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 7,
            symbols: vec![],
            module_path: String::new(),
        }];
        let groups = format_source_locations_by_symbol(&locs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].symbol, "");
        assert_eq!(groups[0].locations.len(), 1);
        assert_eq!(groups[0].locations[0].file, "src/cli.rs");
        assert_eq!(groups[0].locations[0].line, 7);
    }

    #[test]
    fn test_format_source_locations_by_symbol_single() {
        use std::path::PathBuf;

        let locs = vec![SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 7,
            symbols: vec!["ModuleInfo".to_string()],
            module_path: String::new(),
        }];
        let groups = format_source_locations_by_symbol(&locs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].symbol, "ModuleInfo");
        assert_eq!(groups[0].locations.len(), 1);
        assert_eq!(groups[0].locations[0].file, "src/cli.rs");
        assert_eq!(groups[0].locations[0].line, 7);
    }

    #[test]
    fn test_format_source_locations_by_symbol_grouped() {
        use std::path::PathBuf;

        // Same symbol from multiple locations
        let locs = vec![
            SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 7,
                symbols: vec!["ModuleInfo".to_string()],
                module_path: String::new(),
            },
            SourceLocation {
                file: PathBuf::from("src/render.rs"),
                line: 12,
                symbols: vec!["ModuleInfo".to_string()],
                module_path: String::new(),
            },
        ];
        let groups = format_source_locations_by_symbol(&locs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].symbol, "ModuleInfo");
        assert_eq!(groups[0].locations.len(), 2);
        // Locations sorted alphabetically
        assert_eq!(groups[0].locations[0].file, "src/cli.rs");
        assert_eq!(groups[0].locations[0].line, 7);
        assert_eq!(groups[0].locations[1].file, "src/render.rs");
        assert_eq!(groups[0].locations[1].line, 12);
    }

    #[test]
    fn test_format_source_locations_by_symbol_multiple_symbols() {
        use std::path::PathBuf;

        // Multiple symbols from same location (multi-import)
        let locs = vec![SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 7,
            symbols: vec!["ModuleInfo".to_string(), "analyze_module".to_string()],
            module_path: String::new(),
        }];
        let groups = format_source_locations_by_symbol(&locs);
        assert_eq!(groups.len(), 2);
        // Symbols in alphabetical order
        assert_eq!(groups[0].symbol, "ModuleInfo");
        assert_eq!(groups[0].locations.len(), 1);
        assert_eq!(groups[1].symbol, "analyze_module");
        assert_eq!(groups[1].locations.len(), 1);
    }

    #[test]
    fn test_format_source_locations_by_symbol_complex() {
        use std::path::PathBuf;

        // Complex case: multiple symbols, multiple locations
        let locs = vec![
            SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 7,
                symbols: vec!["ModuleInfo".to_string(), "analyze_module".to_string()],
                module_path: String::new(),
            },
            SourceLocation {
                file: PathBuf::from("src/render.rs"),
                line: 12,
                symbols: vec!["ModuleInfo".to_string()],
                module_path: String::new(),
            },
        ];
        let groups = format_source_locations_by_symbol(&locs);
        assert_eq!(groups.len(), 2);
        // ModuleInfo: 2 locations
        assert_eq!(groups[0].symbol, "ModuleInfo");
        assert_eq!(groups[0].locations.len(), 2);
        assert_eq!(groups[0].locations[0].file, "src/cli.rs");
        assert_eq!(groups[0].locations[1].file, "src/render.rs");
        // analyze_module: 1 location
        assert_eq!(groups[1].symbol, "analyze_module");
        assert_eq!(groups[1].locations.len(), 1);
        assert_eq!(groups[1].locations[0].file, "src/cli.rs");
    }

    // === Registry / Module Order Tests ===

    #[test]
    fn test_all_registry_modules_embedded() {
        let config = RenderConfig::default();
        let ir = LayoutIR::new();
        let script = render_script(&config, &ir, &[], &HashSet::new());

        // Registry must contain all 12 modules
        assert!(
            MODULES.len() >= 12,
            "Expected at least 12 modules in registry, got {}",
            MODULES.len()
        );

        // Every module from the registry must appear in the script output
        for module in MODULES {
            let annotation = format!("// @module {}", module.name);
            assert!(
                script.contains(&annotation),
                "Registry module '{}' not found in render_script() output.",
                module.name
            );
        }
    }

    #[test]
    fn test_module_order_deps_before_dependents() {
        let config = RenderConfig::default();
        let ir = LayoutIR::new();
        let script = render_script(&config, &ir, &[], &HashSet::new());

        // Collect positions of each module annotation in the output
        let positions: Vec<(&str, usize)> = MODULES
            .iter()
            .map(|m| {
                let pattern = format!("// @module {}", m.name);
                let pos = script
                    .find(&pattern)
                    .unwrap_or_else(|| panic!("Module '{}' not found in script output", m.name));
                (m.name, pos)
            })
            .collect();

        // SvgScript must be last module (highest position)
        let svg_script_pos = positions.iter().find(|(n, _)| *n == "SvgScript").unwrap().1;
        for (name, pos) in &positions {
            if *name != "SvgScript" {
                assert!(
                    *pos < svg_script_pos,
                    "{} (pos {}) must appear before SvgScript (pos {})",
                    name,
                    pos,
                    svg_script_pos
                );
            }
        }

        // STATIC_DATA (Rust-generated) must appear before all registry modules
        let static_data_pos = script.find("const STATIC_DATA").unwrap();
        for (name, pos) in &positions {
            assert!(
                static_data_pos < *pos,
                "STATIC_DATA must appear before {} (pos {})",
                name,
                pos
            );
        }
    }

    // === STATIC_DATA Tests ===

    #[test]
    fn test_static_data_basic_structure() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "test_crate".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "test_mod".into(),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // STATIC_DATA must exist
        assert!(
            script.contains("const STATIC_DATA = {"),
            "Script should contain STATIC_DATA declaration"
        );
        // Must have nodes and arcs objects
        assert!(
            script.contains("nodes: {"),
            "STATIC_DATA should have nodes object"
        );
        assert!(
            script.contains("arcs: {"),
            "STATIC_DATA should have arcs object"
        );
    }

    #[test]
    fn test_static_data_node_properties() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "test_crate".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "test_mod".into(),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Node 0 (crate) should have type "crate", parent null, hasChildren true
        assert!(script.contains(r#""0": {"#), "Should have node 0");
        assert!(
            script.contains(r#"type: "crate""#),
            "Crate node should have type 'crate'"
        );
        assert!(
            script.contains("parent: null"),
            "Crate node should have parent null"
        );
        assert!(
            script.contains("hasChildren: true"),
            "Parent node should have hasChildren: true"
        );

        // Node 1 (module) should have type "module", parent "0", hasChildren false
        assert!(script.contains(r#""1": {"#), "Should have node 1");
        assert!(
            script.contains(r#"type: "module""#),
            "Module node should have type 'module'"
        );
        assert!(
            script.contains(r#"parent: "0""#),
            "Module node should have parent '0'"
        );
        assert!(
            script.contains("hasChildren: false"),
            "Leaf node should have hasChildren: false"
        );
    }

    #[test]
    fn test_static_data_node_positions() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "test_crate".into());

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::new();

        let script = render_script(&config, &ir, &positioned, &parents);

        // Node should have x and y coordinates
        assert!(script.contains("x: "), "Node should have x coordinate");
        assert!(script.contains("y: "), "Node should have y coordinate");
    }

    #[test]
    fn test_static_data_arc_properties() {
        use std::path::PathBuf;

        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.edges.push(
            LayoutEdge::new(a, b, EdgeContext::production()).with_source_locations(vec![
                SourceLocation {
                    file: PathBuf::from("src/a.rs"),
                    line: 5,
                    symbols: vec!["MyStruct".to_string()],
                    module_path: String::new(),
                },
            ]),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Arc should have from, to, context, usages
        assert!(script.contains(r#""1-2": {"#), "Should have arc 1-2");
        assert!(script.contains(r#"from: "1""#), "Arc should have from");
        assert!(script.contains(r#"to: "2""#), "Arc should have to");
        assert!(
            script.contains(r#"context: { kind: "production", subKind: null, features: [] }"#),
            "Arc should have production context"
        );
        assert!(script.contains("usages: ["), "Arc should have usages array");
        assert!(
            script.contains(r#"symbol: "MyStruct""#),
            "Usages should contain symbol"
        );
        assert!(
            script.contains(r#"file: "src/a.rs""#),
            "Usages should contain file"
        );
        assert!(script.contains("line: 5"), "Usages should contain line");
    }

    #[test]
    fn test_static_data_arc_context_field() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.edges.push(LayoutEdge::new(
            a,
            b,
            EdgeContext::test(crate::model::TestKind::Unit),
        ));

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        assert!(
            script.contains(r#"context: { kind: "test", subKind: "unit", features: [] }"#),
            "Test arc should have context with kind test"
        );
    }

    #[test]
    fn test_static_data_arc_empty_usages() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Arc without source locations should have empty usages array
        assert!(
            script.contains("usages: []"),
            "Arc without locations should have empty usages array"
        );
    }

    #[test]
    fn test_static_data_usages_structured() {
        use std::path::PathBuf;

        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.edges.push(
            LayoutEdge::new(a, b, EdgeContext::production()).with_source_locations(vec![
                SourceLocation {
                    file: PathBuf::from("src/a.rs"),
                    line: 5,
                    symbols: vec!["Symbol1".to_string()],
                    module_path: String::new(),
                },
                SourceLocation {
                    file: PathBuf::from("src/b.rs"),
                    line: 10,
                    symbols: vec!["Symbol1".to_string()],
                    module_path: String::new(),
                },
            ]),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Usages should be array of objects, not array of strings
        assert!(script.contains("usages: ["), "Should have usages array");
        assert!(
            script.contains(r#"symbol: "Symbol1""#),
            "Should have symbol field"
        );
        assert!(
            script.contains("modulePath: null"),
            "Should have modulePath field"
        );
        assert!(
            script.contains("locations: ["),
            "Should have locations array"
        );
        assert!(
            script.contains(r#"file: "src/a.rs""#),
            "Should have file field"
        );
        assert!(script.contains("line: 5"), "Should have line field");
        // Should NOT contain pipe-separated string format
        assert!(
            !script.contains("Symbol1  ← src/a.rs:5"),
            "Should NOT use old string format"
        );
    }

    #[test]
    fn test_static_data_valid_js_syntax() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "test".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "mod".into(),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // STATIC_DATA should be first (before IIFE) and end with semicolon
        let static_data_pos = script.find("const STATIC_DATA").unwrap();
        let iife_pos = script.find("(function()").unwrap_or(usize::MAX);

        assert!(
            static_data_pos < iife_pos,
            "STATIC_DATA should appear before IIFE"
        );

        // Should end with }};
        assert!(
            script.contains("};"),
            "STATIC_DATA should end with semicolon"
        );
    }

    #[test]
    fn test_static_data_empty_ir() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let positioned: Vec<PositionedItem> = vec![];
        let parents: HashSet<NodeId> = HashSet::new();

        let script = render_script(&config, &ir, &positioned, &parents);

        // Empty IR should produce empty nodes and arcs (multiline format)
        assert!(
            script.contains("nodes: {\n  },"),
            "Empty IR should have empty nodes object"
        );
        assert!(
            script.contains("arcs: {\n  }"),
            "Empty IR should have empty arcs object"
        );
    }

    #[test]
    fn test_static_data_escapes_quotes() {
        use std::path::PathBuf;

        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.edges.push(
            LayoutEdge::new(a, b, EdgeContext::production()).with_source_locations(vec![
                SourceLocation {
                    file: PathBuf::from("src/a.rs"),
                    line: 5,
                    symbols: vec!["Test\"Quote".to_string()],
                    module_path: String::new(),
                },
            ]),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Quotes in symbols should be escaped
        assert!(
            script.contains(r#"Test\"Quote"#),
            "Quotes in symbols should be escaped"
        );
    }

    #[test]
    fn test_static_data_contains_classes() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let positioned: Vec<PositionedItem> = vec![];
        let parents: HashSet<NodeId> = HashSet::new();

        let script = render_script(&config, &ir, &positioned, &parents);

        // STATIC_DATA must contain a classes object
        assert!(
            script.contains("classes: {"),
            "STATIC_DATA should have classes object"
        );
        // Spot-check: some key classes should be present with camelCase keys
        assert!(script.contains("depArc:"), "classes should contain depArc");
        assert!(
            script.contains("highlightedArc:"),
            "classes should contain highlightedArc"
        );
        assert!(
            script.contains("selectedCrate:"),
            "classes should contain selectedCrate"
        );
        assert!(
            script.contains("hiddenByFilter:"),
            "classes should contain hiddenByFilter"
        );
        assert!(
            script.contains("collapseToggle:"),
            "classes should contain collapseToggle"
        );
    }

    #[test]
    fn test_static_data_contains_group_member_class() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let positioned: Vec<PositionedItem> = vec![];
        let parents: HashSet<NodeId> = HashSet::new();

        let script = render_script(&config, &ir, &positioned, &parents);

        assert!(
            script.contains(&format!(
                "groupMember: \"{}\"",
                CSS.node_selection.group_member
            )),
            "classes should contain groupMember with value from CSS.node_selection.group_member"
        );
    }

    #[test]
    fn test_static_data_classes_match_css() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let positioned: Vec<PositionedItem> = vec![];
        let parents: HashSet<NodeId> = HashSet::new();

        let script = render_script(&config, &ir, &positioned, &parents);

        // Values in STATIC_DATA.classes must match CSS.* constants
        assert!(
            script.contains(&format!("depArc: \"{}\"", CSS.direction.dep_arc)),
            "depArc value should match CSS.direction.dep_arc"
        );
        assert!(
            script.contains(&format!(
                "highlightedArc: \"{}\"",
                CSS.relation.highlighted_arc
            )),
            "highlightedArc value should match CSS.relation.highlighted_arc"
        );
        assert!(
            script.contains(&format!(
                "selectedCrate: \"{}\"",
                CSS.node_selection.selected_crate
            )),
            "selectedCrate value should match CSS.node_selection.selected_crate"
        );
        assert!(
            script.contains(&format!("collapsed: \"{}\"", CSS.nodes.collapsed)),
            "collapsed value should match CSS.nodes.collapsed"
        );
        assert!(
            script.contains(&format!("virtualArc: \"{}\"", CSS.direction.virtual_arc)),
            "virtualArc value should match CSS.direction.virtual_arc"
        );
    }

    // === Struct / Helper Tests ===

    #[test]
    fn test_symbol_usage_group_creation() {
        // Test struct creation with empty locations
        let group = SymbolUsageGroup {
            symbol: "TestSymbol".to_string(),
            module_path: String::new(),
            locations: vec![],
        };
        assert_eq!(group.symbol, "TestSymbol");
        assert_eq!(group.locations.len(), 0);

        // Test with populated locations
        let group_with_locs = SymbolUsageGroup {
            symbol: "AnotherSymbol".to_string(),
            module_path: String::new(),
            locations: vec![
                UsageLocation {
                    file: "src/main.rs".to_string(),
                    line: 42,
                },
                UsageLocation {
                    file: "src/lib.rs".to_string(),
                    line: 100,
                },
            ],
        };
        assert_eq!(group_with_locs.locations.len(), 2);
        assert_eq!(group_with_locs.locations[0].file, "src/main.rs");
        assert_eq!(group_with_locs.locations[0].line, 42);
    }

    #[test]
    fn test_format_returns_structured_groups() {
        use std::path::PathBuf;

        // Test with 2+ symbols and bare locations
        let locs = vec![
            SourceLocation {
                file: PathBuf::from("src/main.rs"),
                line: 10,
                symbols: vec!["Symbol1".to_string()],
                module_path: String::new(),
            },
            SourceLocation {
                file: PathBuf::from("src/lib.rs"),
                line: 20,
                symbols: vec!["Symbol1".to_string()],
                module_path: String::new(),
            },
            SourceLocation {
                file: PathBuf::from("src/util.rs"),
                line: 30,
                symbols: vec!["Symbol2".to_string()],
                module_path: String::new(),
            },
            SourceLocation {
                file: PathBuf::from("src/bare.rs"),
                line: 40,
                symbols: vec![], // Bare location
                module_path: String::new(),
            },
        ];

        let groups = format_source_locations_by_symbol(&locs);

        // Should have 3 groups: 1 bare (symbol=""), 2 named symbols
        assert_eq!(groups.len(), 3);

        // First group: bare locations (symbol="")
        assert_eq!(groups[0].symbol, "");
        assert_eq!(groups[0].locations.len(), 1);
        assert_eq!(groups[0].locations[0].file, "src/bare.rs");
        assert_eq!(groups[0].locations[0].line, 40);

        // Second group: Symbol1 (2 locations)
        assert_eq!(groups[1].symbol, "Symbol1");
        assert_eq!(groups[1].locations.len(), 2);
        assert_eq!(groups[1].locations[0].file, "src/lib.rs");
        assert_eq!(groups[1].locations[0].line, 20);
        assert_eq!(groups[1].locations[1].file, "src/main.rs");
        assert_eq!(groups[1].locations[1].line, 10);

        // Third group: Symbol2 (1 location)
        assert_eq!(groups[2].symbol, "Symbol2");
        assert_eq!(groups[2].locations.len(), 1);
        assert_eq!(groups[2].locations[0].file, "src/util.rs");
        assert_eq!(groups[2].locations[0].line, 30);
    }

    #[test]
    fn test_render_script_has_collapse_functions() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let script = render_script(&config, &ir, &[], &HashSet::new());
        assert!(
            script.contains("toggleCollapse"),
            "Script should contain toggleCollapse function"
        );
        assert!(
            script.contains("getDescendants"),
            "Script should contain getDescendants function"
        );
        assert!(
            script.contains("relayout"),
            "Script should contain relayout function"
        );
        assert!(
            script.contains("appState"),
            "Script should contain appState for unified state management"
        );
    }

    #[test]
    fn test_render_script_has_hover_functions() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let script = render_script(&config, &ir, &[], &HashSet::new());
        assert!(
            script.contains("AppState.create()"),
            "Script should use AppState module"
        );
        assert!(
            script.contains("handleMouseEnter"),
            "Script should contain handleMouseEnter function"
        );
        assert!(
            script.contains("handleMouseLeave"),
            "Script should contain handleMouseLeave function"
        );
        assert!(
            script.contains("mouseenter"),
            "Script should register mouseenter events"
        );
        assert!(
            script.contains("mouseleave"),
            "Script should register mouseleave events"
        );
    }

    #[test]
    fn test_render_script_has_toggle_deselect() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let script = render_script(&config, &ir, &[], &HashSet::new());
        assert!(
            script.contains("AppState.togglePinned(appState, type, id)"),
            "toggleHighlight should use AppState.togglePinned"
        );
    }

    #[test]
    fn test_render_edge_source_locations_in_static_data() {
        use std::path::PathBuf;

        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        ir.edges.push(
            LayoutEdge::new(a, b, EdgeContext::production()).with_source_locations(vec![
                SourceLocation {
                    file: PathBuf::from("src/a.rs"),
                    line: 5,
                    symbols: vec![],
                    module_path: String::new(),
                },
            ]),
        );
        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);
        let script = render_script(&config, &ir, &positioned, &parents);
        // Source locations are in STATIC_DATA usages array (structured format)
        assert!(script.contains(r#"file: "src/a.rs""#));
        assert!(script.contains("line: 5"));
        assert!(script.contains("usages: ["));
    }

    #[test]
    fn test_render_script_arc_hover_shows_sidebar() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let script = render_script(&config, &ir, &[], &HashSet::new());
        assert!(script.contains("showTransient"));
    }

    #[test]
    fn test_render_script_virtual_arc_aggregates_locations() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let script = render_script(&config, &ir, &[], &HashSet::new());
        assert!(
            script.contains("aggregatedLocations") || script.contains("hiddenEdgeData"),
            "Script should collect locations from hidden edges for virtual arcs"
        );
    }

    #[test]
    fn test_static_data_cycle_info() {
        // Graph with a cycle: A → B → C → A (cycle_ids=[0])
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "b".into(),
        );
        let m_c = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "m_c".into(),
        );
        // Cycle edges with cycle_ids=[0]
        ir.edges.push(
            LayoutEdge::new(a, b, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Transitive, vec![0]),
        );
        ir.edges.push(
            LayoutEdge::new(b, m_c, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Transitive, vec![0]),
        );
        ir.edges.push(
            LayoutEdge::new(m_c, a, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Transitive, vec![0]),
        );
        // Non-cycle edge (no cycle_ids)
        ir.edges
            .push(LayoutEdge::new(a, m_c, EdgeContext::production()));

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // STATIC_DATA must have a cycles array
        assert!(
            script.contains("cycles: ["),
            "STATIC_DATA should have cycles array"
        );

        // Cycle 0 should list the 3 nodes involved
        assert!(
            script.contains(r#""1""#) && script.contains(r#""2""#) && script.contains(r#""3""#),
            "Cycle should reference node IDs 1, 2, 3"
        );

        // Cycle 0 should list the arc IDs
        assert!(
            script.contains(r#""1-2""#)
                && script.contains(r#""2-3""#)
                && script.contains(r#""3-1""#),
            "Cycle should reference arc IDs"
        );

        // Cycle arcs should have cycleIds field in their arc entry
        // Find the arc entry for "1-2" and check it has cycleIds: [0]
        let arc_section_start = script.find("arcs: {").expect("should have arcs section");
        let arc_section = &script[arc_section_start..];
        let arc_1_2_pos = arc_section.find(r#""1-2""#).expect("should have arc 1-2");
        let arc_1_2_region = &arc_section[arc_1_2_pos..arc_1_2_pos + 200];
        assert!(
            arc_1_2_region.contains("cycleIds: [0]"),
            "Cycle arc 1-2 should have cycleIds: [0], got: {}",
            arc_1_2_region
        );

        // Non-cycle arc should NOT have cycleIds
        let arc_1_3_pos = arc_section.find(r#""1-3""#).expect("should have arc 1-3");
        let arc_1_3_region =
            &arc_section[arc_1_3_pos..arc_1_3_pos + 200.min(arc_section.len() - arc_1_3_pos)];
        assert!(
            !arc_1_3_region.contains("cycleIds:"),
            "Non-cycle arc 1-3 should NOT have cycleIds, got: {}",
            arc_1_3_region
        );
    }

    #[test]
    fn test_static_data_multi_cycle_ids() {
        // Graph with overlapping cycles: B↔C (cycle 0) + B↔D (cycle 1)
        // Edge B→C belongs to cycle 0, edge B→D belongs to cycle 1,
        // and a shared edge could belong to both.
        let mut ir = LayoutIR::new();
        let crt = ir.add_item(ItemKind::Crate, "c".into());
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: crt,
            },
            "b".into(),
        );
        let c = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: crt,
            },
            "c".into(),
        );
        let d = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: crt,
            },
            "d".into(),
        );
        // B→C in cycle 0 only
        ir.edges.push(
            LayoutEdge::new(b, c, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![0]),
        );
        // C→B in cycle 0 only
        ir.edges.push(
            LayoutEdge::new(c, b, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![0]),
        );
        // B→D in cycle 1 only
        ir.edges.push(
            LayoutEdge::new(b, d, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![1]),
        );
        // D→B in cycle 1 only
        ir.edges.push(
            LayoutEdge::new(d, b, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![1]),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Arc B→C should have cycleIds: [0]
        let arc_section_start = script.find("arcs: {").expect("should have arcs section");
        let arc_section = &script[arc_section_start..];
        let arc_bc_pos = arc_section
            .find(r#""1-2""#)
            .expect("should have arc 1-2 (B→C)");
        let arc_bc_region = &arc_section[arc_bc_pos..arc_bc_pos + 200];
        assert!(
            arc_bc_region.contains("cycleIds: [0]"),
            "Arc B→C should have cycleIds: [0], got: {}",
            arc_bc_region
        );

        // Arc B→D should have cycleIds: [1]
        let arc_bd_pos = arc_section
            .find(r#""1-3""#)
            .expect("should have arc 1-3 (B→D)");
        let arc_bd_region = &arc_section[arc_bd_pos..arc_bd_pos + 200];
        assert!(
            arc_bd_region.contains("cycleIds: [1]"),
            "Arc B→D should have cycleIds: [1], got: {}",
            arc_bd_region
        );

        // Cycles array should have 2 entries (cycle 0 and cycle 1)
        let cycles_start = script.find("cycles: [").expect("should have cycles array");
        let cycles_section = &script[cycles_start..];
        let cycle_entries: Vec<&str> = cycles_section
            .match_indices("nodes: [")
            .map(|(i, _)| &cycles_section[i..])
            .collect();
        assert_eq!(
            cycle_entries.len(),
            2,
            "Should have exactly 2 cycle entries, got: {}",
            cycle_entries.len()
        );
    }

    #[test]
    fn test_static_data_edge_in_two_cycles() {
        // Edge that belongs to two cycles simultaneously
        let mut ir = LayoutIR::new();
        let crt = ir.add_item(ItemKind::Crate, "c".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: crt,
            },
            "a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: crt,
            },
            "b".into(),
        );
        // Edge A→B belongs to both cycle 0 and cycle 2
        ir.edges.push(
            LayoutEdge::new(a, b, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![0, 2]),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Arc should have cycleIds: [0, 2] (JS array with both IDs)
        let arc_section_start = script.find("arcs: {").expect("should have arcs section");
        let arc_section = &script[arc_section_start..];
        let arc_pos = arc_section.find(r#""1-2""#).expect("should have arc 1-2");
        let arc_region = &arc_section[arc_pos..arc_pos + 200];
        assert!(
            arc_region.contains("cycleIds: [0, 2]"),
            "Arc in two cycles should have cycleIds: [0, 2], got: {}",
            arc_region
        );
    }
}
