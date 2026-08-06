use super::constants::{CSS, LAYOUT, RenderConfig};
use super::positioning::PositionedItem;
use crate::diagnose::ConsumerLocality;
use crate::layout::{CyclicEdgeInfo, ItemKind, LayoutIR, NodeId};
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
    clusters: BTreeMap<String, ClusterData>,
    symbol_localities: BTreeMap<String, BTreeMap<String, SymbolLocalityData>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expand_level: Option<usize>,
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
    nesting: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scc_id: Option<usize>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    scc_id: Option<usize>,
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
    /// True when every location carrying this symbol is a `pub use` re-export.
    /// The cycle view drops these: a re-exported name is not logic coupling,
    /// so it is not part of the cycle (ADR-022).
    #[serde(skip_serializing_if = "is_false")]
    via_reexport: bool,
    locations: Vec<UsageLocation>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde skip_serializing_if signature
fn is_false(b: &bool) -> bool {
    !*b
}

/// A single usage location (file + line number)
#[derive(Serialize)]
struct UsageLocation {
    file: String,
    line: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CycleData {
    nodes: Vec<String>,
    arcs: Vec<String>,
    scc_id: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClusterData {
    #[serde(rename = "crate")]
    crate_name: String,
    module_count: usize,
    cycle_count: usize,
    cycles: Vec<Vec<CycleArcData>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CycleArcData {
    from_id: String,
    to_id: String,
    symbols: usize,
}

fn cycle_arc_data(edge: &CyclicEdgeInfo) -> CycleArcData {
    CycleArcData {
        from_id: edge.from_id.to_string(),
        to_id: edge.to_id.to_string(),
        symbols: edge.symbols,
    }
}

/// Consumer locality of one symbol of a provider. `module` names the common
/// home for `singleConsumer`/`commonAncestor`; absent for `crateWide`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SymbolLocalityData {
    locality: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<String>,
    consumers: Vec<String>,
}

/// Accumulates a cycle's nodes, arcs, and SCC id while iterating edges.
#[derive(Default)]
struct CycleAccum {
    nodes: BTreeSet<NodeId>,
    arcs: BTreeSet<String>,
    scc_id: Option<usize>,
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
    // A symbol counts as re-exported only when every location carrying it is a
    // `pub use`; a single real import makes it coupling. Mirrors the edge-level
    // `all(via_reexport)` rule (graph.rs `is_reexport_module_dep`).
    let mut reexport_by_symbol: BTreeMap<String, bool> = BTreeMap::new();

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
                let flag = reexport_by_symbol.entry(symbol.clone()).or_insert(true);
                *flag &= loc.via_reexport;
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
            via_reexport: false,
            locations: bare_locations
                .into_iter()
                .map(|(file, line)| UsageLocation { file, line })
                .collect(),
        });
    }

    // Then: symbol-grouped locations in alphabetical order
    for (symbol, locations) in by_symbol {
        let via_reexport = reexport_by_symbol.get(&symbol).copied().unwrap_or(false);
        groups.push(SymbolUsageGroup {
            symbol,
            module_path: module_path_opt.clone(),
            via_reexport,
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
    config: &RenderConfig,
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
            ItemKind::ExternalSection => "external-section",
            ItemKind::ExternalCrate {
                is_direct_dependency: true,
                ..
            } => "external",
            ItemKind::ExternalCrate {
                is_direct_dependency: false,
                ..
            } => "external-transitive",
        };
        let parent = match &item.kind {
            ItemKind::Crate | ItemKind::ExternalSection => None,
            ItemKind::Module { parent, .. } | ItemKind::ExternalCrate { parent, .. } => {
                Some(parent.to_string())
            }
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
                nesting: super::positioning::item_nesting(&item.kind),
                version: item.version.clone(),
                scc_id: item.scc_id,
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
                scc_id: edge.scc_id,
            },
        );
    }

    let mut cycle_map: BTreeMap<usize, CycleAccum> = BTreeMap::new();
    for edge in &ir.edges {
        for &cid in &edge.cycle_ids {
            let entry = cycle_map.entry(cid).or_default();
            entry.nodes.insert(edge.from);
            entry.nodes.insert(edge.to);
            entry.arcs.insert(format!("{}-{}", edge.from, edge.to));
            entry.scc_id = entry.scc_id.or(edge.scc_id);
        }
    }
    let cycles: Vec<CycleData> = cycle_map
        .into_values()
        .map(|accum| CycleData {
            nodes: accum
                .nodes
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            arcs: accum.arcs.into_iter().collect(),
            scc_id: accum.scc_id.expect("every cycle lies in an SCC"),
        })
        .collect();

    let classes: BTreeMap<String, String> = [
        ("crateNode", CSS.nodes.crate_node),
        ("module", CSS.nodes.module),
        ("externalSection", CSS.nodes.external_section),
        ("externalCrate", CSS.nodes.external_crate),
        ("externalTransitive", CSS.nodes.external_transitive),
        ("label", CSS.nodes.label),
        ("treeLine", CSS.nodes.tree_line),
        ("collapseToggle", CSS.nodes.collapse_toggle),
        ("collapsed", CSS.nodes.collapsed),
        ("depArc", CSS.direction.dep_arc),
        ("downward", CSS.direction.downward),
        ("upward", CSS.direction.upward),
        ("depArrow", CSS.direction.dep_arrow),
        ("upwardArrow", CSS.direction.upward_arrow),
        ("cycleArc", CSS.direction.cycle_arc),
        ("cycleArrow", CSS.direction.cycle_arrow),
        ("clusterModeOn", CSS.relation.cluster_mode_on),
        ("arcHitarea", CSS.direction.arc_hitarea),
        ("crateDepArc", CSS.direction.crate_dep_arc),
        ("moduleDepArc", CSS.direction.module_dep_arc),
        ("reexportArc", CSS.direction.reexport_arc),
        ("virtualArc", CSS.direction.virtual_arc),
        ("virtualArrow", CSS.direction.virtual_arrow),
        ("virtualHitarea", CSS.direction.virtual_hitarea),
        ("selectedCrate", CSS.node_selection.selected_crate),
        ("selectedModule", CSS.node_selection.selected_module),
        ("selectedExternal", CSS.node_selection.selected_external),
        (
            "selectedExternalTransitive",
            CSS.node_selection.selected_external_transitive,
        ),
        ("groupMember", CSS.node_selection.group_member),
        ("cycleMember", CSS.node_selection.cycle_member),
        ("highlightedArc", CSS.relation.highlighted_arc),
        ("highlightedArrow", CSS.relation.highlighted_arrow),
        ("highlightedLabel", CSS.relation.highlighted_label),
        ("depNode", CSS.relation.dep_node),
        ("dependentNode", CSS.relation.dependent_node),
        ("dimmed", CSS.relation.dimmed),
        ("hasHighlight", CSS.relation.has_highlight),
        ("hasPinned", CSS.relation.has_pinned),
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
        ("toolbarDropdown", CSS.toolbar.dropdown),
        ("toolbarDropdownBtn", CSS.toolbar.dropdown_btn),
        ("toolbarDropdownPanel", CSS.toolbar.dropdown_panel),
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

    let clusters: BTreeMap<String, ClusterData> = ir
        .clusters
        .iter()
        .map(|(&scc, c)| {
            (
                scc.to_string(),
                ClusterData {
                    crate_name: c.crate_name.clone(),
                    module_count: c.module_count,
                    cycle_count: c.cycle_count,
                    cycles: c
                        .cycles
                        .iter()
                        .map(|block| block.iter().map(cycle_arc_data).collect())
                        .collect(),
                },
            )
        })
        .collect();

    let symbol_localities: BTreeMap<String, BTreeMap<String, SymbolLocalityData>> = ir
        .symbol_localities
        .iter()
        .map(|(&provider, localities)| {
            let per_symbol = localities
                .iter()
                .map(|(symbol, sl)| {
                    let (locality, module) = match sl.locality {
                        ConsumerLocality::SingleConsumer(n) => {
                            ("singleConsumer", Some(n.to_string()))
                        }
                        ConsumerLocality::CommonAncestor(m) => {
                            ("commonAncestor", Some(m.to_string()))
                        }
                        ConsumerLocality::CrateWide => ("crateWide", None),
                    };
                    (
                        symbol.clone(),
                        SymbolLocalityData {
                            locality,
                            module,
                            consumers: sl.consumers.iter().map(ToString::to_string).collect(),
                        },
                    )
                })
                .collect();
            (provider.to_string(), per_symbol)
        })
        .collect();

    let data = StaticData {
        nodes,
        arcs,
        cycles,
        classes,
        clusters,
        symbol_localities,
        expand_level: config.expand_level,
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
    let static_data = generate_static_data(config, ir, positioned, parents);

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
    use crate::diagnose::RepresentativeCycles;
    use crate::graph::{ArcGraph, Edge, Node, Reexports};
    use crate::layout::{LayoutEdge, build_layout};
    use crate::model::{EdgeContext, SourceLocation};

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
            via_reexport: false,
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
            via_reexport: false,
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
                via_reexport: false,
            },
            SourceLocation {
                file: PathBuf::from("src/render.rs"),
                line: 12,
                symbols: vec!["ModuleInfo".to_string()],
                module_path: String::new(),
                via_reexport: false,
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
            via_reexport: false,
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
    fn test_format_marks_symbol_reexport_only_when_all_locations_reexport() {
        use std::path::PathBuf;

        let locs = vec![
            // Foo: every location is a re-export -> flagged
            SourceLocation {
                file: PathBuf::from("src/a.rs"),
                line: 1,
                symbols: vec!["Foo".to_string()],
                module_path: String::new(),
                via_reexport: true,
            },
            SourceLocation {
                file: PathBuf::from("src/b.rs"),
                line: 2,
                symbols: vec!["Foo".to_string()],
                module_path: String::new(),
                via_reexport: true,
            },
            // Bar: one re-export, one real import -> coupling, not flagged
            SourceLocation {
                file: PathBuf::from("src/c.rs"),
                line: 3,
                symbols: vec!["Bar".to_string()],
                module_path: String::new(),
                via_reexport: true,
            },
            SourceLocation {
                file: PathBuf::from("src/d.rs"),
                line: 4,
                symbols: vec!["Bar".to_string()],
                module_path: String::new(),
                via_reexport: false,
            },
        ];
        let groups = format_source_locations_by_symbol(&locs);
        let group = |name: &str| groups.iter().find(|g| g.symbol == name).unwrap();
        assert!(group("Foo").via_reexport, "all-reexport symbol is flagged");
        assert!(
            !group("Bar").via_reexport,
            "a single real import clears the flag"
        );
    }

    #[test]
    fn test_static_data_usage_via_reexport_in_json() {
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
                    symbols: vec!["Reexported".to_string()],
                    module_path: String::new(),
                    via_reexport: true,
                },
                SourceLocation {
                    file: PathBuf::from("src/a.rs"),
                    line: 6,
                    symbols: vec!["Coupled".to_string()],
                    module_path: String::new(),
                    via_reexport: false,
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

        let usages = data["arcs"]["1-2"]["usages"].as_array().unwrap();
        let find = |name: &str| {
            usages
                .iter()
                .find(|u| u["symbol"] == name)
                .unwrap_or_else(|| panic!("symbol {name} missing"))
        };
        // true is serialized; false is skipped (absent), so JS reads it as falsy.
        assert_eq!(find("Reexported")["viaReexport"], true);
        assert!(find("Coupled")["viaReexport"].is_null());
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
                via_reexport: false,
            },
            SourceLocation {
                file: PathBuf::from("src/render.rs"),
                line: 12,
                symbols: vec!["ModuleInfo".to_string()],
                module_path: String::new(),
                via_reexport: false,
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
                    via_reexport: false,
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
                    via_reexport: false,
                },
                SourceLocation {
                    file: PathBuf::from("src/b.rs"),
                    line: 10,
                    symbols: vec!["Symbol1".to_string()],
                    module_path: String::new(),
                    via_reexport: false,
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
                    via_reexport: false,
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
            classes.contains_key("selectedExternal"),
            "classes should contain selectedExternal"
        );
        assert!(
            classes.contains_key("hiddenByFilter"),
            "classes should contain hiddenByFilter"
        );
        assert!(
            classes.contains_key("collapseToggle"),
            "classes should contain collapseToggle"
        );
        assert!(
            classes.contains_key("externalSection"),
            "classes should contain externalSection"
        );
        assert!(
            classes.contains_key("externalCrate"),
            "classes should contain externalCrate"
        );
        assert!(
            classes.contains_key("externalTransitive"),
            "classes should contain externalTransitive"
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
        assert_eq!(
            data["classes"]["selectedExternal"],
            CSS.node_selection.selected_external
        );
        assert_eq!(data["classes"]["collapsed"], CSS.nodes.collapsed);
        assert_eq!(data["classes"]["virtualArc"], CSS.direction.virtual_arc);
        assert_eq!(
            data["classes"]["externalSection"],
            CSS.nodes.external_section
        );
        assert_eq!(data["classes"]["externalCrate"], CSS.nodes.external_crate);
        assert_eq!(
            data["classes"]["externalTransitive"],
            CSS.nodes.external_transitive
        );
    }

    // === Struct / Helper Tests ===

    #[test]
    fn test_symbol_usage_group_creation() {
        // Test struct creation with empty locations
        let group = SymbolUsageGroup {
            symbol: "TestSymbol".to_string(),
            module_path: None,
            via_reexport: false,
            locations: vec![],
        };
        assert_eq!(group.symbol, "TestSymbol");
        assert_eq!(group.locations.len(), 0);

        // Test with populated locations
        let group_with_locs = SymbolUsageGroup {
            symbol: "AnotherSymbol".to_string(),
            module_path: None,
            via_reexport: false,
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
                via_reexport: false,
            },
            SourceLocation {
                file: PathBuf::from("src/lib.rs"),
                line: 20,
                symbols: vec!["Symbol1".to_string()],
                module_path: String::new(),
                via_reexport: false,
            },
            SourceLocation {
                file: PathBuf::from("src/util.rs"),
                line: 30,
                symbols: vec!["Symbol2".to_string()],
                module_path: String::new(),
                via_reexport: false,
            },
            SourceLocation {
                file: PathBuf::from("src/bare.rs"),
                line: 40,
                symbols: vec![], // Bare location
                module_path: String::new(),
                via_reexport: false,
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
            script.contains("AppState.toggleSelection(appState, type, id)"),
            "toggleHighlight should use AppState.toggleSelection"
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
                    via_reexport: false,
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
        // Cycle nodes a, b, m_c are in SCC 0; the crate node is not.
        ir.items[a].scc_id = Some(0);
        ir.items[b].scc_id = Some(0);
        ir.items[m_c].scc_id = Some(0);
        // Cycle edges with cycle_ids=[0], all in SCC 0
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Transitive,
                vec![0],
                0,
            ));
        ir.edges.push(
            LayoutEdge::new(b, m_c, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Transitive,
                vec![0],
                0,
            ),
        );
        ir.edges.push(
            LayoutEdge::new(m_c, a, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Transitive,
                vec![0],
                0,
            ),
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

        // Cycle carries its SCC id.
        assert_eq!(cycles[0]["sccId"], serde_json::json!(0));

        // Cycle arc "1-2" and cycle node "1" carry sccId 0.
        assert_eq!(data["arcs"]["1-2"]["sccId"], serde_json::json!(0));
        assert_eq!(data["nodes"]["1"]["sccId"], serde_json::json!(0));

        // Non-cycle arc "1-3" and the crate node "0" have no sccId.
        assert!(data["arcs"]["1-3"].get("sccId").is_none());
        assert!(data["nodes"]["0"].get("sccId").is_none());
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
        // B<->C and B<->D share node B, so all four edges form one SCC (id 0).
        // B->C in cycle 0 only
        ir.edges
            .push(LayoutEdge::new(b, c, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Direct,
                vec![0],
                0,
            ));
        // C->B in cycle 0 only
        ir.edges
            .push(LayoutEdge::new(c, b, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Direct,
                vec![0],
                0,
            ));
        // B->D in cycle 1 only
        ir.edges
            .push(LayoutEdge::new(b, d, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Direct,
                vec![1],
                0,
            ));
        // D->B in cycle 1 only
        ir.edges
            .push(LayoutEdge::new(d, b, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Direct,
                vec![1],
                0,
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

        // Arc B->C should have cycleIds: [0]
        assert_eq!(data["arcs"]["1-2"]["cycleIds"], serde_json::json!([0]));

        // Arc B->D should have cycleIds: [1]
        assert_eq!(data["arcs"]["1-3"]["cycleIds"], serde_json::json!([1]));

        // Cycles array should have 2 entries
        let cycles = data["cycles"].as_array().expect("cycles is array");
        assert_eq!(cycles.len(), 2);

        // Two elementary cycles, one SCC: both carry sccId 0, as do both arcs.
        assert_eq!(cycles[0]["sccId"], serde_json::json!(0));
        assert_eq!(cycles[1]["sccId"], serde_json::json!(0));
        assert_eq!(data["arcs"]["1-2"]["sccId"], serde_json::json!(0));
        assert_eq!(data["arcs"]["1-3"]["sccId"], serde_json::json!(0));
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Direct,
                vec![0, 2],
                0,
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

        assert_eq!(data["arcs"]["1-2"]["cycleIds"], serde_json::json!([0, 2]));
    }

    #[test]
    fn test_static_data_clusters() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "app".into());
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
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Direct,
                vec![0],
                0,
            ));
        ir.edges
            .push(LayoutEdge::new(b, a, EdgeContext::production()).with_cycle(
                crate::layout::CycleKind::Direct,
                vec![0],
                0,
            ));
        ir.clusters.insert(
            0,
            crate::layout::ClusterInfo {
                crate_name: "app".into(),
                module_count: 2,
                cycle_count: 1,
                cycles: vec![vec![
                    crate::layout::CyclicEdgeInfo {
                        from_id: a,
                        to_id: b,
                        symbols: 2,
                    },
                    crate::layout::CyclicEdgeInfo {
                        from_id: b,
                        to_id: a,
                        symbols: 1,
                    },
                ]],
            },
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

        let cl = &data["clusters"]["0"];
        assert_eq!(cl["crate"], "app");
        assert_eq!(cl["moduleCount"], 2);
        assert_eq!(cl["cycleCount"], 1);
        let cycles = cl["cycles"].as_array().unwrap();
        assert_eq!(cycles.len(), 1, "one block for the one cycle");
        let block = cycles[0].as_array().unwrap();
        assert_eq!(block.len(), 2, "closing edge included");
        assert_eq!(block[0]["fromId"], a.to_string());
        assert_eq!(block[0]["toId"], b.to_string());
        assert_eq!(block[0]["symbols"], 2);
        assert_eq!(block[1]["fromId"], b.to_string());
        assert_eq!(block[1]["toId"], a.to_string());
        assert_eq!(block[1]["symbols"], 1);
        assert!(cl.get("edges").is_none());
        assert!(cl.get("toBreak").is_none());
        // The edge's arc-id addresses a serialized arc.
        assert!(data["arcs"][format!("{a}-{b}")].is_object());
    }

    #[test]
    fn test_static_data_tangle_cluster_two_edges_ranked_and_addressable() {
        // Two 2-cycles sharing module "a" (a<->b, a<->c): one SCC, two feedback
        // edges. Each lies on exactly one cycle, so ranking (ADR-021: cycles
        // desc, then symbols asc) is decided by the symbol count, not by name —
        // b->a (1 symbol) must rank before c->a (2 symbols).
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        let mods: Vec<_> = ["a", "b", "c"]
            .iter()
            .map(|&name| {
                let idx = graph.add_node(Node::Module {
                    name: name.into(),
                    crate_idx,
                });
                graph.add_edge(crate_idx, idx, Edge::Contains);
                idx
            })
            .collect();
        let (a, b, c) = (mods[0], mods[1], mods[2]);
        let locations = |symbols: usize| {
            // One symbol per line, so the edge reads the same whether the count
            // takes sites or symbols.
            (0..symbols)
                .map(|i| SourceLocation {
                    file: "src/lib.rs".into(),
                    line: i + 1,
                    symbols: vec![format!("Sym{i}")],
                    module_path: String::new(),
                    via_reexport: false,
                })
                .collect::<Vec<_>>()
        };
        for (from, to, symbols) in [(a, b, 5), (b, a, 1), (a, c, 3), (c, a, 2)] {
            graph.add_edge(
                from,
                to,
                Edge::ModuleDep {
                    locations: locations(symbols),
                    context: EdgeContext::production(),
                },
            );
        }

        let analysis = graph
            .production_subgraph(Reexports::Excluded)
            .representative_cycles();
        let ir = build_layout(&graph, &analysis, Reexports::Excluded);

        assert_eq!(ir.clusters.len(), 1, "expected exactly one tangle cluster");

        let id_of = |name: &str| ir.items.iter().find(|item| item.label == name).unwrap().id;
        let (a_id, b_id, c_id) = (id_of("a"), id_of("b"), id_of("c"));

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = ir
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Module { parent, .. } | ItemKind::ExternalCrate { parent, .. } => {
                    Some(*parent)
                }
                ItemKind::Crate | ItemKind::ExternalSection => None,
            })
            .collect();
        let script = render_script(&config, &ir, &positioned, &parents);
        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");

        let clusters = data["clusters"].as_object().unwrap();
        assert_eq!(clusters.len(), 1);

        let cluster = clusters.values().next().unwrap();
        assert!(cluster.get("edges").is_none());
        assert!(cluster.get("toBreak").is_none());
        let cycles = cluster["cycles"].as_array().unwrap();
        assert_eq!(cycles.len(), 2, "one block per elementary cycle");

        // Both cycles start at "a" (shared node); tie-break by rest-sequence
        // rank puts a<->b before a<->c since b's layout rank precedes c's.
        let block0 = cycles[0].as_array().unwrap();
        assert_eq!(block0.len(), 2, "closing edge included");
        assert_eq!(block0[0]["fromId"], a_id.to_string());
        assert_eq!(block0[0]["toId"], b_id.to_string());
        assert_eq!(block0[0]["symbols"], 5);
        assert_eq!(block0[1]["fromId"], b_id.to_string());
        assert_eq!(block0[1]["toId"], a_id.to_string());
        assert_eq!(block0[1]["symbols"], 1);

        let block1 = cycles[1].as_array().unwrap();
        assert_eq!(block1.len(), 2, "closing edge included");
        assert_eq!(block1[0]["fromId"], a_id.to_string());
        assert_eq!(block1[0]["toId"], c_id.to_string());
        assert_eq!(block1[0]["symbols"], 3);
        assert_eq!(block1[1]["fromId"], c_id.to_string());
        assert_eq!(block1[1]["toId"], a_id.to_string());
        assert_eq!(block1[1]["symbols"], 2);

        // Every block edge's fromId-toId addresses a serialized arc.
        for edge in block0.iter().chain(block1.iter()) {
            let arc_id = format!(
                "{}-{}",
                edge["fromId"].as_str().unwrap(),
                edge["toId"].as_str().unwrap()
            );
            assert!(data["arcs"][&arc_id].is_object(), "arc {arc_id} missing");
        }
    }

    /// Render `ir` and parse the embedded `STATIC_DATA` JSON.
    fn static_data_json(ir: &LayoutIR) -> serde_json::Value {
        let config = RenderConfig::default();
        let positioned = calculate_positions(ir, &config, calculate_box_width(ir));
        let parents: HashSet<NodeId> = ir
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Module { parent, .. } | ItemKind::ExternalCrate { parent, .. } => {
                    Some(*parent)
                }
                ItemKind::Crate | ItemKind::ExternalSection => None,
            })
            .collect();
        let script = render_script(&config, ir, &positioned, &parents);
        let json_str = script
            .split("const STATIC_DATA = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        serde_json::from_str(json_str).expect("valid JSON")
    }

    /// Build a flat `app` crate with the given child modules; returns the graph.
    fn crate_with(modules: &[&str]) -> ArcGraph {
        let mut graph = ArcGraph::new();
        let crate_idx = graph.add_node(Node::Crate {
            name: "app".into(),
            path: "/app".into(),
        });
        for &name in modules {
            let idx = graph.add_node(Node::Module {
                name: name.into(),
                crate_idx,
            });
            graph.add_edge(crate_idx, idx, Edge::Contains);
        }
        graph
    }

    /// Find a module's `NodeIndex` by name.
    fn module_idx(graph: &ArcGraph, name: &str) -> petgraph::graph::NodeIndex {
        graph
            .node_indices()
            .find(|&i| matches!(&graph[i], Node::Module { name: n, .. } if n == name))
            .unwrap()
    }

    /// Add a production `ModuleDep` `from -> to` carrying `symbols` at one site.
    fn prod_dep_syms(graph: &mut ArcGraph, from: &str, to: &str, symbols: &[&str]) {
        let (src, dst) = (module_idx(graph, from), module_idx(graph, to));
        graph.add_edge(
            src,
            dst,
            Edge::ModuleDep {
                locations: vec![SourceLocation {
                    file: format!("src/{from}.rs").into(),
                    line: 1,
                    symbols: symbols.iter().map(|s| (*s).to_owned()).collect(),
                    module_path: String::new(),
                    via_reexport: false,
                }],
                context: EdgeContext::production(),
            },
        );
    }

    #[test]
    fn test_static_data_symbol_locality_single_consumer() {
        // model provides Foo only to user: single consumer, home = user.
        let mut graph = crate_with(&["model", "user"]);
        prod_dep_syms(&mut graph, "user", "model", &["Foo"]);

        let analysis = graph
            .production_subgraph(Reexports::Excluded)
            .representative_cycles();
        let ir = build_layout(&graph, &analysis, Reexports::Excluded);
        let id_of = |name: &str| ir.items.iter().find(|it| it.label == name).unwrap().id;
        let (model_id, user_id) = (id_of("model"), id_of("user"));

        let data = static_data_json(&ir);
        let sl = &data["symbolLocalities"][model_id.to_string()]["Foo"];
        assert_eq!(sl["locality"], "singleConsumer");
        assert_eq!(sl["module"], user_id.to_string());
        assert_eq!(sl["consumers"], serde_json::json!([user_id.to_string()]));
    }

    /// Add a child module `child` under existing module/crate `parent`.
    fn add_module(graph: &mut ArcGraph, parent: &str, child: &str) {
        let parent_idx = module_idx(graph, parent);
        let crate_idx = match &graph[parent_idx] {
            Node::Module { crate_idx, .. } => *crate_idx,
            _ => parent_idx,
        };
        let child_idx = graph.add_node(Node::Module {
            name: child.into(),
            crate_idx,
        });
        graph.add_edge(parent_idx, child_idx, Edge::Contains);
    }

    fn symbol_ids(value: &serde_json::Value) -> BTreeSet<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn test_static_data_symbol_locality_common_ancestor() {
        // parser & reexport (both under analyze) import Foo from model:
        // common ancestor analyze, provider outside -> home = analyze.
        let mut graph = crate_with(&["model", "analyze"]);
        add_module(&mut graph, "analyze", "parser");
        add_module(&mut graph, "analyze", "reexport");
        prod_dep_syms(&mut graph, "parser", "model", &["Foo"]);
        prod_dep_syms(&mut graph, "reexport", "model", &["Foo"]);

        let analysis = graph
            .production_subgraph(Reexports::Excluded)
            .representative_cycles();
        let ir = build_layout(&graph, &analysis, Reexports::Excluded);
        let id_of = |name: &str| ir.items.iter().find(|it| it.label == name).unwrap().id;
        let model_id = id_of("model");

        let data = static_data_json(&ir);
        let sl = &data["symbolLocalities"][model_id.to_string()]["Foo"];
        assert_eq!(sl["locality"], "commonAncestor");
        assert_eq!(sl["module"], id_of("analyze").to_string());
        assert_eq!(
            symbol_ids(&sl["consumers"]),
            [id_of("parser").to_string(), id_of("reexport").to_string()]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn test_static_data_symbol_locality_crate_wide() {
        // Two top-level consumers, no shared module ancestor -> crate-wide, no home.
        let mut graph = crate_with(&["model", "user", "admin"]);
        prod_dep_syms(&mut graph, "user", "model", &["Foo"]);
        prod_dep_syms(&mut graph, "admin", "model", &["Foo"]);

        let analysis = graph
            .production_subgraph(Reexports::Excluded)
            .representative_cycles();
        let ir = build_layout(&graph, &analysis, Reexports::Excluded);
        let id_of = |name: &str| ir.items.iter().find(|it| it.label == name).unwrap().id;

        let data = static_data_json(&ir);
        let sl = &data["symbolLocalities"][id_of("model").to_string()]["Foo"];
        assert_eq!(sl["locality"], "crateWide");
        assert!(sl.get("module").is_none(), "crate-wide has no home");
        assert_eq!(
            symbol_ids(&sl["consumers"]),
            [id_of("user").to_string(), id_of("admin").to_string()]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn test_static_data_symbol_locality_reexport_only_absent() {
        // A pure re-export edge is republication, not use: no locality entry.
        let mut graph = crate_with(&["model", "user"]);
        let (user, model) = (module_idx(&graph, "user"), module_idx(&graph, "model"));
        graph.add_edge(
            user,
            model,
            Edge::ModuleDep {
                locations: vec![SourceLocation {
                    file: "src/user.rs".into(),
                    line: 1,
                    symbols: vec!["Foo".to_owned()],
                    module_path: String::new(),
                    via_reexport: true,
                }],
                context: EdgeContext::production(),
            },
        );

        let analysis = graph
            .production_subgraph(Reexports::Excluded)
            .representative_cycles();
        let ir = build_layout(&graph, &analysis, Reexports::Excluded);
        let data = static_data_json(&ir);
        assert!(data["symbolLocalities"].as_object().unwrap().is_empty());
    }
}
