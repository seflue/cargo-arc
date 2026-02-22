use super::constants::{CSS, LAYOUT, RenderConfig};
use super::positioning::PositionedItem;
use crate::layout::{ItemKind, LayoutIR, NodeId};
use crate::model::SourceLocation;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};

include!(concat!(env!("OUT_DIR"), "/js_modules.rs"));

// === Serialization structs ===

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StaticData {
    nodes: BTreeMap<String, NodeData>,
    arcs: BTreeMap<String, ArcData>,
    cycles: Vec<CycleData>,
    classes: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeData {
    #[serde(rename = "type")]
    node_type: &'static str,
    name: String,
    parent: Option<String>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    has_children: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArcData {
    from: String,
    to: String,
    context: ArcContext,
    usages: Vec<SymbolUsageGroup>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cycle_ids: Vec<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArcContext {
    kind: String,
    sub_kind: Option<String>,
    features: Vec<String>,
}

impl From<&crate::model::EdgeContext> for ArcContext {
    fn from(ctx: &crate::model::EdgeContext) -> Self {
        Self {
            kind: ctx.kind.kind_js().to_string(),
            sub_kind: ctx.kind.sub_kind_js().map(String::from),
            features: ctx.features.clone(),
        }
    }
}

/// A group of usage locations for a single symbol
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SymbolUsageGroup {
    symbol: String,
    module_path: Option<String>,
    locations: Vec<UsageLocation>,
}

/// A single usage location (file + line number)
#[derive(Serialize)]
struct UsageLocation {
    file: String,
    line: usize,
}

#[derive(Serialize)]
struct CycleData {
    nodes: Vec<String>,
    arcs: Vec<String>,
}

// === Data building ===

/// Format source locations grouped by symbol.
///
/// Inverts the Location->Symbols structure to Symbol->Locations for structured display.
///
/// Returns a Vec of `SymbolUsageGroup` objects. Bare locations (without symbols)
/// are returned with symbol="". Groups are ordered: bare locations first, then
/// symbol groups alphabetically.
fn format_source_locations_by_symbol(locs: &[SourceLocation]) -> Vec<SymbolUsageGroup> {
    if locs.is_empty() {
        return Vec::new();
    }

    let module_path = locs
        .first()
        .map(|l| l.module_path.clone())
        .unwrap_or_default();
    let module_path_opt = if module_path.is_empty() {
        None
    } else {
        Some(module_path)
    };

    // Invert: Symbol -> Vec<(file, line)>
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
            module_path: module_path_opt.clone(),
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
            module_path: module_path_opt.clone(),
            locations: locations
                .into_iter()
                .map(|(file, line)| UsageLocation { file, line })
                .collect(),
        });
    }

    groups
}

/// Generate `STATIC_DATA` JavaScript constant from layout data
#[allow(clippy::too_many_lines)] // single cohesive serialization function
fn generate_static_data(
    ir: &LayoutIR,
    positioned: &[PositionedItem],
    parents: &HashSet<NodeId>,
) -> String {
    let mut nodes = BTreeMap::new();
    for pos in positioned {
        let item = &ir.items[pos.id];
        let node_type = match &item.kind {
            ItemKind::Crate => "crate",
            ItemKind::Module { .. } => "module",
        };
        let parent = match &item.kind {
            ItemKind::Crate => None,
            ItemKind::Module { parent, .. } => Some(parent.to_string()),
        };
        nodes.insert(
            pos.id.to_string(),
            NodeData {
                node_type,
                name: item.label.clone(),
                parent,
                x: pos.x,
                y: pos.y,
                width: pos.width,
                height: pos.height,
                has_children: parents.contains(&pos.id),
            },
        );
    }

    let mut arcs = BTreeMap::new();
    for edge in &ir.edges {
        let arc_id = format!("{}-{}", edge.from, edge.to);
        let usages = format_source_locations_by_symbol(&edge.source_locations);
        arcs.insert(
            arc_id,
            ArcData {
                from: edge.from.to_string(),
                to: edge.to.to_string(),
                context: ArcContext::from(&edge.context),
                usages,
                cycle_ids: edge.cycle_ids.clone(),
            },
        );
    }

    let mut cycle_map: BTreeMap<usize, (BTreeSet<NodeId>, BTreeSet<String>)> = BTreeMap::new();
    for edge in &ir.edges {
        for &cid in &edge.cycle_ids {
            let entry = cycle_map.entry(cid).or_default();
            entry.0.insert(edge.from);
            entry.0.insert(edge.to);
            entry.1.insert(format!("{}-{}", edge.from, edge.to));
        }
    }
    let cycles: Vec<CycleData> = cycle_map
        .into_values()
        .map(|(nodes, arcs)| CycleData {
            nodes: nodes.iter().map(std::string::ToString::to_string).collect(),
            arcs: arcs.into_iter().collect(),
        })
        .collect();

    let classes: BTreeMap<String, String> = [
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
        ("moduleDepArc", CSS.direction.module_dep_arc),
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
        ("glowCycle", CSS.relation.glow_cycle),
        ("viewOptions", CSS.toolbar.view_options),
        ("toolbarBtn", CSS.toolbar.btn),
        ("toolbarCheckbox", CSS.toolbar.checkbox),
        ("checked", CSS.toolbar.checked),
        ("toolbarRoot", CSS.toolbar.root),
        ("toolbarHtmlBtn", CSS.toolbar.html_btn),
        ("toolbarToggle", CSS.toolbar.toggle),
        ("toolbarScopeBtn", CSS.toolbar.scope_btn),
        ("toolbarScopeActive", CSS.toolbar.scope_active),
        ("toolbarResultCount", CSS.toolbar.result_count),
        ("searchActive", CSS.search.search_active),
        ("searchMatch", CSS.search.search_match),
        ("searchMatchParent", CSS.search.search_match_parent),
        ("arcCount", CSS.labels.arc_count),
        ("arcCountBg", CSS.labels.arc_count_bg),
        ("arcCountGroup", CSS.labels.arc_count_group),
        ("hiddenByFilter", CSS.labels.hidden_by_filter),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let data = StaticData {
        nodes,
        arcs,
        cycles,
        classes,
    };
    format!(
        "const STATIC_DATA = {};",
        serde_json::to_string(&data).expect("StaticData serialization cannot fail")
    )
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
            let placeholder = format!("__{key}__");
            let value = match *key {
                "ROW_HEIGHT" => config.row_height.to_string(),
                "MARGIN" => config.margin.to_string(),
                "TOOLBAR_HEIGHT" => LAYOUT.toolbar.height.to_string(),
                "SIDEBAR_SHADOW_PAD" => LAYOUT.sidebar.shadow_padding().to_string(),
                other => panic!("Unknown config key: {other}"),
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
                    "{name} (pos {pos}) must appear before SvgScript (pos {svg_script_pos})"
                );
            }
        }

        // STATIC_DATA (Rust-generated) must appear before all registry modules
        let static_data_pos = script.find("const STATIC_DATA").unwrap();
        for (name, pos) in &positions {
            assert!(
                static_data_pos < *pos,
                "STATIC_DATA must appear before {name} (pos {pos})"
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
        // Must have nodes and arcs keys (JSON quoted)
        assert!(
            script.contains(r#""nodes""#),
            "STATIC_DATA should have nodes key"
        );
        assert!(
            script.contains(r#""arcs""#),
            "STATIC_DATA should have arcs key"
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

        // Parse STATIC_DATA as JSON to verify structure
        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        // Node 0 (crate)
        let node0 = &data["nodes"]["0"];
        assert_eq!(node0["type"], "crate");
        assert_eq!(node0["name"], "test_crate");
        assert!(node0["parent"].is_null());
        assert_eq!(node0["hasChildren"], true);

        // Node 1 (module)
        let node1 = &data["nodes"]["1"];
        assert_eq!(node1["type"], "module");
        assert_eq!(node1["name"], "test_mod");
        assert_eq!(node1["parent"], "0");
        assert_eq!(node1["hasChildren"], false);
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
        assert!(script.contains(r#""x""#), "Node should have x coordinate");
        assert!(script.contains(r#""y""#), "Node should have y coordinate");
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        let arc = &data["arcs"]["1-2"];
        assert_eq!(arc["from"], "1");
        assert_eq!(arc["to"], "2");
        assert_eq!(arc["context"]["kind"], "production");
        assert!(arc["context"]["subKind"].is_null());
        assert_eq!(arc["context"]["features"], serde_json::json!([]));
        assert_eq!(arc["usages"][0]["symbol"], "MyStruct");
        assert_eq!(arc["usages"][0]["locations"][0]["file"], "src/a.rs");
        assert_eq!(arc["usages"][0]["locations"][0]["line"], 5);
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        let arc = &data["arcs"]["1-2"];
        assert_eq!(arc["context"]["kind"], "test");
        assert_eq!(arc["context"]["subKind"], "unit");
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        let arc = &data["arcs"]["1-2"];
        assert_eq!(arc["usages"], serde_json::json!([]));
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        let arc = &data["arcs"]["1-2"];
        let usages = arc["usages"].as_array().expect("usages is array");
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0]["symbol"], "Symbol1");
        assert!(usages[0]["modulePath"].is_null());
        let locations = usages[0]["locations"]
            .as_array()
            .expect("locations is array");
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0]["file"], "src/a.rs");
        assert_eq!(locations[0]["line"], 5);
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

        // Should end with };
        assert!(
            script.contains("};"),
            "STATIC_DATA should end with semicolon"
        );

        // The JSON portion must be valid JSON
        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        serde_json::from_str::<serde_json::Value>(json_str)
            .expect("STATIC_DATA must be valid JSON");
    }

    #[test]
    fn test_static_data_empty_ir() {
        let ir = LayoutIR::new();
        let config = RenderConfig::default();
        let positioned: Vec<PositionedItem> = vec![];
        let parents: HashSet<NodeId> = HashSet::new();

        let script = render_script(&config, &ir, &positioned, &parents);

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        // Empty IR should produce empty nodes and arcs
        assert_eq!(data["nodes"], serde_json::json!({}));
        assert_eq!(data["arcs"], serde_json::json!({}));
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

        // serde_json escapes quotes correctly
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        let classes = data["classes"].as_object().expect("classes is object");
        assert!(
            classes.contains_key("depArc"),
            "classes should contain depArc"
        );
        assert!(
            classes.contains_key("highlightedArc"),
            "classes should contain highlightedArc"
        );
        assert!(
            classes.contains_key("selectedCrate"),
            "classes should contain selectedCrate"
        );
        assert!(
            classes.contains_key("hiddenByFilter"),
            "classes should contain hiddenByFilter"
        );
        assert!(
            classes.contains_key("collapseToggle"),
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        assert_eq!(
            data["classes"]["groupMember"], CSS.node_selection.group_member,
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        assert_eq!(data["classes"]["depArc"], CSS.direction.dep_arc);
        assert_eq!(
            data["classes"]["highlightedArc"],
            CSS.relation.highlighted_arc
        );
        assert_eq!(
            data["classes"]["selectedCrate"],
            CSS.node_selection.selected_crate
        );
        assert_eq!(data["classes"]["collapsed"], CSS.nodes.collapsed);
        assert_eq!(data["classes"]["virtualArc"], CSS.direction.virtual_arc);
    }

    // === Struct / Helper Tests ===

    #[test]
    fn test_symbol_usage_group_creation() {
        // Test struct creation with empty locations
        let group = SymbolUsageGroup {
            symbol: "TestSymbol".to_string(),
            module_path: None,
            locations: vec![],
        };
        assert_eq!(group.symbol, "TestSymbol");
        assert_eq!(group.locations.len(), 0);

        // Test with populated locations
        let group_with_locs = SymbolUsageGroup {
            symbol: "AnotherSymbol".to_string(),
            module_path: None,
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        let arc = &data["arcs"]["1-2"];
        let usages = arc["usages"].as_array().expect("usages is array");
        assert_eq!(usages[0]["locations"][0]["file"], "src/a.rs");
        assert_eq!(usages[0]["locations"][0]["line"], 5);
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
        // Graph with a cycle: A -> B -> C -> A (cycle_ids=[0])
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

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        // Cycles array should exist with one cycle
        let cycles = data["cycles"].as_array().expect("cycles is array");
        assert_eq!(cycles.len(), 1);

        // Cycle 0 should list the 3 nodes involved
        let cycle_nodes = cycles[0]["nodes"].as_array().unwrap();
        assert!(cycle_nodes.contains(&serde_json::json!("1")));
        assert!(cycle_nodes.contains(&serde_json::json!("2")));
        assert!(cycle_nodes.contains(&serde_json::json!("3")));

        // Cycle 0 should list the arc IDs
        let cycle_arcs = cycles[0]["arcs"].as_array().unwrap();
        assert!(cycle_arcs.contains(&serde_json::json!("1-2")));
        assert!(cycle_arcs.contains(&serde_json::json!("2-3")));
        assert!(cycle_arcs.contains(&serde_json::json!("3-1")));

        // Cycle arc "1-2" should have cycleIds: [0]
        assert_eq!(data["arcs"]["1-2"]["cycleIds"], serde_json::json!([0]));

        // Non-cycle arc "1-3" should NOT have cycleIds
        assert!(
            data["arcs"]["1-3"].get("cycleIds").is_none(),
            "Non-cycle arc 1-3 should NOT have cycleIds"
        );
    }

    #[test]
    fn test_static_data_multi_cycle_ids() {
        // Graph with overlapping cycles: B<->C (cycle 0) + B<->D (cycle 1)
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
        // B->C in cycle 0 only
        ir.edges.push(
            LayoutEdge::new(b, c, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![0]),
        );
        // C->B in cycle 0 only
        ir.edges.push(
            LayoutEdge::new(c, b, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![0]),
        );
        // B->D in cycle 1 only
        ir.edges.push(
            LayoutEdge::new(b, d, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![1]),
        );
        // D->B in cycle 1 only
        ir.edges.push(
            LayoutEdge::new(d, b, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![1]),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        // Arc B->C should have cycleIds: [0]
        assert_eq!(data["arcs"]["1-2"]["cycleIds"], serde_json::json!([0]));

        // Arc B->D should have cycleIds: [1]
        assert_eq!(data["arcs"]["1-3"]["cycleIds"], serde_json::json!([1]));

        // Cycles array should have 2 entries
        let cycles = data["cycles"].as_array().expect("cycles is array");
        assert_eq!(cycles.len(), 2);
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
        // Edge A->B belongs to both cycle 0 and cycle 2
        ir.edges.push(
            LayoutEdge::new(a, b, EdgeContext::production())
                .with_cycle(crate::layout::CycleKind::Direct, vec![0, 2]),
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        assert_eq!(data["arcs"]["1-2"]["cycleIds"], serde_json::json!([0, 2]));
    }
}
