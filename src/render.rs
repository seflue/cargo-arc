//! SVG Generation

use crate::graph::SourceLocation;
use crate::layout::{EdgeKind, ItemKind, LayoutIR, NodeId};
use std::collections::HashSet;

include!(concat!(env!("OUT_DIR"), "/js_modules.rs"));

/// All layout constants consolidated in one place.
/// Use `static` (not `const`) so references like `let tb = &LAYOUT.toolbar` work.
pub(crate) struct LayoutConstants {
    pub char_width: f32,
    pub box_padding: f32,
    pub arc_base: f32,
    pub arc_scale: f32,
    pub arrow_length: f32,
    pub tooltip_padding: f32,
    pub tooltip_offset: f32,
    pub tooltip_line_height: f32,
    pub crate_height: f32,
    pub module_height: f32,
    pub tree_line_x_offset: f32,
    pub crate_border_radius: f32,
    pub module_border_radius: f32,
    pub text_padding_x: f32,
    pub text_y_offset: f32,
    pub toggle_offset: f32,
    pub toggle_y_offset: f32,
    pub arc_y_offset: f32,
    pub arc_min_space: f32,
    pub toolbar: ToolbarLayout,
}

pub(crate) struct ToolbarLayout {
    pub height: f32,
    pub btn_x: f32,
    pub btn_y: f32,
    pub btn_width: f32,
    pub btn_height: f32,
    pub separator_spacing: f32,
    pub separator_y1: f32,
    pub separator_y2: f32,
    pub cb_spacing: f32,
    pub cb_y: f32,
    pub cb_size: f32,
    pub label_x_offset: f32,
    pub label_y_offset: f32,
    pub cb2_x_offset: f32,
}

static LAYOUT: LayoutConstants = LayoutConstants {
    char_width: 7.2,
    box_padding: 20.0,
    arc_base: 20.0,
    arc_scale: 15.0,
    arrow_length: 8.0,
    tooltip_padding: 6.0,
    tooltip_offset: 10.0,
    tooltip_line_height: 14.0,
    crate_height: 24.0,
    module_height: 20.0,
    tree_line_x_offset: 10.0,
    crate_border_radius: 3.0,
    module_border_radius: 2.0,
    text_padding_x: 10.0,
    text_y_offset: 4.0,
    toggle_offset: 14.0,
    toggle_y_offset: 4.0,
    arc_y_offset: 3.0,
    arc_min_space: 50.0,
    toolbar: ToolbarLayout {
        height: 40.0,
        btn_x: 10.0,
        btn_y: 8.0,
        btn_width: 80.0,
        btn_height: 24.0,
        separator_spacing: 15.0,
        separator_y1: 8.0,
        separator_y2: 32.0,
        cb_spacing: 15.0,
        cb_y: 12.0,
        cb_size: 16.0,
        label_x_offset: 6.0,
        label_y_offset: 4.0,
        cb2_x_offset: 190.0,
    },
};

/// Calculate text width based on character count
fn calculate_text_width(text: &str) -> f32 {
    text.len() as f32 * LAYOUT.char_width
}

/// Calculate uniform box width from longest label in LayoutIR
fn calculate_box_width(ir: &LayoutIR) -> f32 {
    ir.items
        .iter()
        .map(|item| calculate_text_width(&item.label))
        .fold(0.0_f32, |a, b| a.max(b))
        + LAYOUT.box_padding
}

/// Separator for tooltip lines (newlines get normalized in XML attributes)
const TOOLTIP_LINE_SEP: &str = "|";

/// Calculate maximum arc width from edges
fn calculate_max_arc_width(positioned: &[PositionedItem], ir: &LayoutIR, row_height: f32) -> f32 {
    ir.edges
        .iter()
        .filter_map(|edge| {
            let from = positioned.iter().find(|p| p.id == edge.from)?;
            let to = positioned.iter().find(|p| p.id == edge.to)?;
            let hops = ((to.y - from.y).abs() / row_height).round().max(1.0);
            Some(LAYOUT.arc_base + hops * LAYOUT.arc_scale + LAYOUT.arrow_length)
        })
        .fold(0.0_f32, |a, b| a.max(b))
}

/// Format source locations grouped by symbol.
///
/// Inverts the Location→Symbols structure to Symbol→Locations for clearer display.
///
/// Example output (column-aligned):
/// ```text
/// ModuleInfo      ← src/cli.rs:7
///                 ← src/render.rs:12
/// analyze_module  ← src/cli.rs:7
/// ```
fn format_source_locations_by_symbol(locs: &[SourceLocation]) -> String {
    use std::collections::BTreeMap;

    if locs.is_empty() {
        return String::new();
    }

    // Invert: Symbol → Vec<file:line>
    let mut by_symbol: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut bare_locations: Vec<String> = Vec::new();

    for loc in locs {
        let file_line = format!("{}:{}", loc.file.display(), loc.line);
        if loc.symbols.is_empty() {
            // Location without symbols - collect separately
            bare_locations.push(file_line);
        } else {
            for symbol in &loc.symbols {
                by_symbol
                    .entry(symbol.clone())
                    .or_default()
                    .push(file_line.clone());
            }
        }
    }

    // Sort locations within each symbol alphabetically
    for locations in by_symbol.values_mut() {
        locations.sort();
    }

    // Find max symbol length for column alignment
    let max_symbol_len = by_symbol.keys().map(|s| s.len()).max().unwrap_or(0);

    let mut lines = Vec::new();

    // First: bare locations (without symbols)
    for loc in bare_locations {
        lines.push(loc);
    }

    // Then: symbol-grouped locations in aligned columns
    for (symbol, locations) in by_symbol {
        for (i, loc) in locations.iter().enumerate() {
            if i == 0 {
                // First line: symbol + padding + arrow + location
                let padding = " ".repeat(max_symbol_len - symbol.len() + 2);
                lines.push(format!("{}{}← {}", symbol, padding, loc));
            } else {
                // Continuation: spaces + arrow + location
                let spaces = " ".repeat(max_symbol_len + 2);
                lines.push(format!("{}← {}", spaces, loc));
            }
        }
    }

    lines.join(TOOLTIP_LINE_SEP)
}

/// Configuration for SVG rendering
#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub row_height: f32,
    pub indent_size: f32,
    pub margin: f32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            row_height: 30.0,
            indent_size: 20.0,
            margin: 20.0,
        }
    }
}

/// Positioned item for rendering
struct PositionedItem {
    id: NodeId,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    label: String,
    kind: ItemKind,
}

/// Render LayoutIR to SVG string
pub fn render(ir: &LayoutIR, config: &RenderConfig) -> String {
    let box_width = calculate_box_width(ir);
    let positioned = calculate_positions(ir, config, box_width);
    let max_arc_width = calculate_max_arc_width(&positioned, ir, config.row_height);
    let (max_tooltip_width, max_tooltip_height) = calculate_max_tooltip_size(ir);
    let (width, height) = calculate_canvas_size(
        &positioned,
        config,
        max_arc_width,
        max_tooltip_width,
        max_tooltip_height,
    );

    // Collect all node IDs that are parents (have children)
    let parents: HashSet<NodeId> = ir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Module { parent, .. } => Some(*parent),
            ItemKind::Crate => None,
        })
        .collect();

    let mut svg = String::new();
    svg.push_str(&render_header(width, height));
    svg.push_str(&render_styles());
    svg.push_str(&render_tree_lines(&positioned, ir));
    svg.push_str(&render_nodes(&positioned, &parents));
    svg.push_str(&render_edges(&positioned, ir, config.row_height));
    svg.push_str(&render_toolbar(width));
    svg.push_str(&render_script(config, ir, &positioned, &parents));
    svg.push_str("</svg>\n");
    svg
}

fn calculate_positions(
    ir: &LayoutIR,
    config: &RenderConfig,
    box_width: f32,
) -> Vec<PositionedItem> {
    ir.items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let nesting = match &item.kind {
                ItemKind::Crate => 0,
                ItemKind::Module { nesting, .. } => *nesting,
            };
            let height = match &item.kind {
                ItemKind::Crate => LAYOUT.crate_height,
                ItemKind::Module { .. } => LAYOUT.module_height,
            };
            PositionedItem {
                id: item.id,
                x: config.margin + (nesting as f32 * config.indent_size),
                y: config.margin + LAYOUT.toolbar.height + (index as f32 * config.row_height),
                width: box_width,
                height,
                label: item.label.clone(),
                kind: item.kind.clone(),
            }
        })
        .collect()
}

/// Calculate tooltip dimensions for the tallest/widest tooltip
fn calculate_max_tooltip_size(ir: &LayoutIR) -> (f32, f32) {
    let (max_width, max_height) = ir
        .edges
        .iter()
        .filter(|e| !e.source_locations.is_empty())
        .map(|edge| {
            let tooltip_text = format_source_locations_by_symbol(&edge.source_locations);
            let line_count = tooltip_text.split(TOOLTIP_LINE_SEP).count();
            let max_line_len = tooltip_text
                .split(TOOLTIP_LINE_SEP)
                .map(|line| line.len())
                .max()
                .unwrap_or(0);
            let width = calculate_text_width(&"x".repeat(max_line_len));
            let height = line_count as f32 * LAYOUT.tooltip_line_height;
            (width, height)
        })
        .fold((0.0_f32, 0.0_f32), |(aw, ah), (w, h)| {
            (aw.max(w), ah.max(h))
        });

    (
        max_width + LAYOUT.tooltip_padding * 2.0 + LAYOUT.tooltip_offset,
        max_height + LAYOUT.tooltip_padding,
    )
}

fn calculate_canvas_size(
    positioned: &[PositionedItem],
    config: &RenderConfig,
    max_arc_width: f32,
    max_tooltip_width: f32,
    max_tooltip_height: f32,
) -> (f32, f32) {
    let base_height = if positioned.is_empty() {
        config.margin * 2.0
    } else {
        config.margin * 2.0 + positioned.len() as f32 * config.row_height
    };
    // Add toolbar height and tooltip height for bottom overflow
    let height = base_height + LAYOUT.toolbar.height + max_tooltip_height;

    // Width: max(box_right_edge) + arc_space + tooltip_width + margin
    let max_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, |a, b| a.max(b));
    // Use calculated max_arc_width, with a minimum buffer for short/no edges
    let arc_space = max_arc_width.max(LAYOUT.arc_min_space);
    let width = max_x + arc_space + max_tooltip_width + config.margin;
    (width, height)
}

fn render_header(width: f32, height: f32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
"#
    )
}

fn render_styles() -> String {
    r#"  <style>
    /* Node base styles - Tailwind fills, Catppuccin strokes */
    .crate { fill: #dbeafe; stroke: #1e66f5; stroke-width: 1.5; }
    .module { fill: #ffedd5; stroke: #fe640b; stroke-width: 1.5; }
    .label { font-family: monospace; font-size: 12px; }
    .tree-line { stroke: #666; stroke-width: 1; }
    /* Arc base styles - Catppuccin Latte colors */
    .dep-arc, .cycle-arc { pointer-events: none; }
    .dep-arc { fill: none; stroke-width: 0.5; }
    .dep-arc.downward { stroke: #40a02b; }  /* Green - normal flow */
    .dep-arc.upward { stroke: #df8e1d; }    /* Yellow - child→parent */
    .dep-arrow { fill: #40a02b; }
    .upward-arrow { fill: #df8e1d; }
    .cycle-arc { fill: none; stroke: #d20f39; stroke-width: 0.5; }  /* Red - cycles */
    .cycle-arrow { fill: #d20f39; }
    /* Hit-area for arc interactions */
    .arc-hitarea { fill: none; stroke: transparent; stroke-width: 12; pointer-events: stroke; cursor: pointer; }
    /* Interactive highlighting - Selected (saturated fills, thicker border) */
    .selected-crate { fill: #93c5fd !important; stroke-width: 3 !important; }
    .selected-module { fill: #fdba74 !important; stroke-width: 3 !important; }
    /* Arc highlighting: marker class only (no color/width override - arc keeps direction color) */
    .highlighted-arc { /* marker class for highlight state */ }
    /* Glow classes for shadow paths (relation color) */
    .glow-incoming { stroke: #40a02b !important; }
    .glow-outgoing { stroke: #8839ef !important; }
    /* Node borders: relation color (green=dep, purple=dependent) */
    .dep-node { stroke: #40a02b !important; stroke-width: 2.5 !important; }
    .dependent-node { stroke: #8839ef !important; stroke-width: 2.5 !important; }
    .dimmed { opacity: 0.3; pointer-events: none; }
    path.dimmed:not(.shadow-path) { stroke: #888 !important; }
    polygon.dimmed { fill: #888 !important; }
    .crate, .module, .dep-arc, .cycle-arc { cursor: pointer; }
    /* Collapse functionality */
    .collapse-toggle { font-family: monospace; font-size: 14px; cursor: pointer; fill: #666; }
    .collapse-toggle:hover { fill: #1e66f5; }
    .collapsed { display: none; }
    .virtual-arc { fill: none; stroke-width: 0.5; stroke-dasharray: 4,2; }
    .virtual-arc.downward { stroke: #40a02b; }
    .virtual-arc.upward { stroke: #df8e1d; }
    .virtual-arrow { cursor: pointer; }
    .virtual-arrow.downward { fill: #40a02b; }
    .virtual-arrow.upward { fill: #df8e1d; }
    .arc-count { font-family: monospace; font-size: 10px; fill: #40a02b; text-anchor: middle; }
    .arc-count-bg { fill: #ffffff; rx: 2; }
    .arc-count.dep-edge { fill: #40a02b !important; font-size: 12px; font-weight: bold; stroke: none !important; }
    .arc-count.dependent-edge { fill: #8839ef !important; font-size: 12px; font-weight: bold; stroke: none !important; }
    .arc-count.dimmed { opacity: 0.3; }
    .child-count { font-size: 10px; fill: #888; }
    /* Shadow path for glow effect */
    .shadow-path { pointer-events: none; stroke-linecap: round; }
    /* Floating label for source locations */
    .floating-label { pointer-events: none; }
    .floating-label rect { fill: #1a1a2e; fill-opacity: 0.95; rx: 4; }
    .floating-label text { fill: #e0e0e0; font-family: monospace; font-size: 11px; }
    /* Toolbar */
    .view-options { cursor: default; }
    .toolbar-btn { fill: #f5f5f5; stroke: #666; rx: 3; cursor: pointer; }
    .toolbar-btn:hover { fill: #e0e0e0; }
    .toolbar-btn-text { font-family: sans-serif; font-size: 11px; text-anchor: middle; }
    .toolbar-checkbox { fill: #fff; stroke: #666; rx: 2; cursor: pointer; }
    .toolbar-checkbox.checked { fill: #1e66f5; }
    .toolbar-label { font-family: sans-serif; font-size: 11px; cursor: pointer; }
    .toolbar-separator { stroke: #ccc; }
    .toolbar-disabled { opacity: 0.4; pointer-events: none; }
    /* Filter visibility */
    .hidden-by-filter { display: none; }
  </style>
"#
    .to_string()
}

fn render_toolbar(width: f32) -> String {
    let mut toolbar = String::new();
    toolbar.push_str("  <g class=\"view-options\">\n");

    // Background rect (full width, toolbar height)
    toolbar.push_str(&format!(
        "    <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#fafafa\" stroke=\"#e0e0e0\"/>\n",
        width, LAYOUT.toolbar.height
    ));

    // Collapse/Expand All button
    let tb = &LAYOUT.toolbar;
    let btn_x = tb.btn_x;
    let btn_y = tb.btn_y;
    let btn_width = tb.btn_width;
    let btn_height = tb.btn_height;
    toolbar.push_str(&format!(
        "    <rect id=\"collapse-toggle-btn\" class=\"toolbar-btn\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        btn_x, btn_y, btn_width, btn_height
    ));
    toolbar.push_str(&format!(
        "    <text id=\"collapse-toggle-label\" class=\"toolbar-btn-text\" x=\"{}\" y=\"{}\" dominant-baseline=\"middle\">Collapse All</text>\n",
        btn_x + btn_width / 2.0,
        btn_y + btn_height / 2.0
    ));

    // Separator
    let sep_x = btn_x + btn_width + tb.separator_spacing;
    toolbar.push_str(&format!(
        "    <line class=\"toolbar-separator\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
        sep_x, tb.separator_y1, sep_x, tb.separator_y2
    ));

    // CrateDep checkbox (checked by default)
    let cb1_x = sep_x + tb.cb_spacing;
    let cb_y = tb.cb_y;
    let cb_size = tb.cb_size;
    toolbar.push_str(&format!(
        "    <rect id=\"crate-dep-checkbox\" class=\"toolbar-checkbox checked\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        cb1_x, cb_y, cb_size, cb_size
    ));
    toolbar.push_str(&format!(
        "    <text class=\"toolbar-label\" x=\"{}\" y=\"{}\">Show Crate Dependencies</text>\n",
        cb1_x + cb_size + tb.label_x_offset,
        cb_y + cb_size / 2.0 + tb.label_y_offset
    ));

    // Tests checkbox (disabled)
    let cb2_x = cb1_x + tb.cb2_x_offset;
    toolbar.push_str(&format!(
        "    <rect class=\"toolbar-checkbox toolbar-disabled\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        cb2_x, cb_y, cb_size, cb_size
    ));
    toolbar.push_str(&format!(
        "    <text class=\"toolbar-label toolbar-disabled\" x=\"{}\" y=\"{}\">Tests</text>\n",
        cb2_x + cb_size + tb.label_x_offset,
        cb_y + cb_size / 2.0 + tb.label_y_offset
    ));

    toolbar.push_str("  </g>\n");
    toolbar
}

/// Generate STATIC_DATA JavaScript constant from layout data
fn generate_static_data(
    ir: &LayoutIR,
    positioned: &[PositionedItem],
    parents: &HashSet<NodeId>,
) -> String {
    let mut lines = Vec::new();
    lines.push("const STATIC_DATA = {".to_string());

    // Generate nodes object
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

        lines.push(format!(
            "    \"{}\": {{ type: \"{}\", parent: {}, x: {}, y: {}, width: {}, height: {}, hasChildren: {} }}{}",
            pos.id, node_type, parent_str, pos.x, pos.y, pos.width, pos.height, has_children, comma
        ));
    }
    lines.push("  },".to_string());

    // Generate arcs object
    lines.push("  arcs: {".to_string());
    for (i, edge) in ir.edges.iter().enumerate() {
        let arc_id = format!("{}-{}", edge.from, edge.to);

        // Format usages from source_locations
        let usages: Vec<String> = if edge.source_locations.is_empty() {
            vec![]
        } else {
            let formatted = format_source_locations_by_symbol(&edge.source_locations);
            formatted
                .split(TOOLTIP_LINE_SEP)
                .map(escape_js_string)
                .collect()
        };

        let usages_str = if usages.is_empty() {
            "[]".to_string()
        } else {
            format!("[\"{}\"]", usages.join("\", \""))
        };

        let comma = if i < ir.edges.len() - 1 { "," } else { "" };

        lines.push(format!(
            "    \"{}\": {{ from: \"{}\", to: \"{}\", usages: {} }}{}",
            arc_id, edge.from, edge.to, usages_str, comma
        ));
    }
    lines.push("  }".to_string());

    lines.push("};".to_string());
    lines.join("\n")
}

/// Escape string for JavaScript (handles quotes and backslashes)
fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_script(
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

fn render_tree_lines(
    positioned: &[PositionedItem],
    ir: &LayoutIR,
) -> String {
    let mut lines = String::new();
    lines.push_str("  <g id=\"tree-lines\">\n");

    // Find children for each parent
    for item in &ir.items {
        if let ItemKind::Module { parent, .. } = &item.kind {
            let parent_pos = positioned.iter().find(|p| p.id == *parent);
            let child_pos = positioned.iter().find(|p| p.id == item.id);

            if let (Some(parent_pos), Some(child_pos)) = (parent_pos, child_pos) {
                let line_x = parent_pos.x + LAYOUT.tree_line_x_offset;
                let parent_bottom = parent_pos.y + parent_pos.height;
                let child_mid_y = child_pos.y + child_pos.height / 2.0;

                let data_attrs = format!(r#" data-parent="{}" data-child="{}""#, parent, item.id);

                lines.push_str(&format!(
                    "    <line class=\"tree-line\" x1=\"{line_x}\" y1=\"{parent_bottom}\" x2=\"{line_x}\" y2=\"{child_mid_y}\"{data_attrs}/>\n"
                ));

                let child_left = child_pos.x;
                lines.push_str(&format!(
                    "    <line class=\"tree-line\" x1=\"{line_x}\" y1=\"{child_mid_y}\" x2=\"{child_left}\" y2=\"{child_mid_y}\"{data_attrs}/>\n"
                ));
            }
        }
    }

    lines.push_str("  </g>\n");
    lines
}

fn render_nodes(positioned: &[PositionedItem], parents: &HashSet<NodeId>) -> String {
    let mut nodes = String::new();
    nodes.push_str("  <g id=\"nodes\">\n");

    for item in positioned {
        let class = match &item.kind {
            ItemKind::Crate => "crate",
            ItemKind::Module { .. } => "module",
        };
        let rx = match &item.kind {
            ItemKind::Crate => LAYOUT.crate_border_radius,
            ItemKind::Module { .. } => LAYOUT.module_border_radius,
        };

        // data-parent attribute for modules
        let parent_attr = match &item.kind {
            ItemKind::Module { parent, .. } => format!(r#" data-parent="{}""#, parent),
            ItemKind::Crate => String::new(),
        };

        // data-has-children attribute for parents
        let has_children_attr = if parents.contains(&item.id) {
            r#" data-has-children="true""#
        } else {
            ""
        };

        let label = escape_xml(&item.label);
        let text_x = item.x + LAYOUT.text_padding_x;
        let text_y = item.y + item.height / 2.0 + LAYOUT.text_y_offset;

        nodes.push_str(&format!(
            "    <rect class=\"{class}\" id=\"node-{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{rx}\"{parent_attr}{has_children_attr}/>\n",
            item.id, item.x, item.y, item.width, item.height
        ));

        // Label with optional child-count tspan for parents
        if parents.contains(&item.id) {
            nodes.push_str(&format!(
                "    <text class=\"label\" x=\"{text_x}\" y=\"{text_y}\">{label}<tspan id=\"count-{}\" class=\"child-count\"></tspan></text>\n",
                item.id
            ));
        } else {
            nodes.push_str(&format!(
                "    <text class=\"label\" x=\"{text_x}\" y=\"{text_y}\">{label}</text>\n"
            ));
        }

        // Toggle icon (+/-) for parents
        if parents.contains(&item.id) {
            let toggle_x = item.x + item.width - LAYOUT.toggle_offset;
            let toggle_y = item.y + item.height / 2.0 + LAYOUT.toggle_y_offset;
            nodes.push_str(&format!(
                "    <text class=\"collapse-toggle\" data-target=\"{}\" x=\"{}\" y=\"{}\">−</text>\n",
                item.id, toggle_x, toggle_y
            ));
        }
    }

    nodes.push_str("  </g>\n");
    nodes
}

fn render_edges(
    positioned: &[PositionedItem],
    ir: &LayoutIR,
    row_height: f32,
) -> String {
    let mut base_arcs = String::new();
    let mut hitareas = String::new();

    // Find the rightmost edge of all nodes for base arc position
    let base_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, |a, b| a.max(b));

    for edge in &ir.edges {
        let from_pos = positioned.iter().find(|p| p.id == edge.from);
        let to_pos = positioned.iter().find(|p| p.id == edge.to);

        if let (Some(from), Some(to)) = (from_pos, to_pos) {
            let from_x = from.x + from.width;
            let to_x = to.x + to.width;

            // Offset arc endpoints: outgoing slightly below center, incoming slightly above
            // This prevents arcs from overlapping at nodes with both incoming and outgoing connections
            let y_offset = LAYOUT.arc_y_offset;
            let from_y = from.y + from.height / 2.0 + y_offset; // outgoing: below center
            let to_y = to.y + to.height / 2.0 - y_offset; // incoming: above center

            // Calculate "hops" - how many rows the arc spans
            let hops = ((to_y - from_y).abs() / row_height).round().max(1.0);

            // Control point X scales with number of hops
            // Base offset + additional offset per hop
            let arc_offset = LAYOUT.arc_base + (hops * LAYOUT.arc_scale);
            let ctrl_x = base_x + arc_offset;
            let mid_y = (from_y + to_y) / 2.0;

            // S-shaped Bezier with two Q commands
            let path = format!(
                "M {from_x},{from_y} Q {ctrl_x},{from_y} {ctrl_x},{mid_y} Q {ctrl_x},{to_y} {to_x},{to_y}"
            );

            let (base_arc_class, arrow_class, extra_style, direction) = match edge.kind {
                EdgeKind::Downward => ("dep-arc downward", "dep-arrow", "", "downward"),
                EdgeKind::Upward => ("dep-arc upward", "upward-arrow", "", "upward"),
                EdgeKind::DirectCycle => ("cycle-arc", "cycle-arrow", "", "cycle"),
                EdgeKind::TransitiveCycle => (
                    "cycle-arc",
                    "cycle-arrow",
                    " stroke-dasharray=\"4,2\"",
                    "cycle",
                ),
            };

            // Add crate-dep-arc class for Crate-to-Crate edges
            let is_crate_dep = matches!((&from.kind, &to.kind), (ItemKind::Crate, ItemKind::Crate));
            let arc_class = if is_crate_dep {
                format!("{} crate-dep-arc", base_arc_class)
            } else {
                base_arc_class.to_string()
            };

            let edge_id = format!("{}-{}", edge.from, edge.to);

            // Hit-area path (invisible, 12px wide, receives pointer events) → hitareas layer
            // Note: source-locations are read from STATIC_DATA in JavaScript, not DOM attributes
            hitareas.push_str(&format!(
                "    <path class=\"arc-hitarea\" data-arc-id=\"{edge_id}\" data-from=\"{}\" data-to=\"{}\" data-direction=\"{direction}\" d=\"{path}\"/>\n",
                edge.from, edge.to
            ));
            // Visible path (styled, no pointer events) → base-arcs layer
            base_arcs.push_str(&format!(
                "    <path class=\"{arc_class}\" id=\"edge-{edge_id}\" data-arc-id=\"{edge_id}\" data-direction=\"{direction}\" d=\"{path}\"{extra_style}/>\n"
            ));

            // Arrow head pointing to target → base-arcs layer
            let arrow = render_arrow(to_x, to_y, arrow_class, &edge_id);
            base_arcs.push_str(&arrow);

            // For DirectCycle, add reverse arrow
            if edge.kind == EdgeKind::DirectCycle {
                let reverse_arrow = render_arrow(from_x, from_y, arrow_class, &edge_id);
                base_arcs.push_str(&reverse_arrow);
            }
        }
    }

    // 6-layer architecture for Z-order guarantees:
    // 1. base-arcs: Non-highlighted arcs + arrows (bottom)
    // 2. base-labels: Non-highlighted labels (JS fills via virtual arcs)
    // 3. highlight-shadows: Shadow/glow paths behind highlighted arcs (JS fills)
    // 4. highlight-arcs: Highlighted arcs + arrows
    // 5. highlight-labels: Highlighted labels
    // 6. hitareas: Transparent hit areas (always on top)
    format!(
        r#"  <g id="base-arcs-layer">
{}  </g>
  <g id="base-labels-layer"></g>
  <g id="highlight-shadows"></g>
  <g id="highlight-arcs-layer"></g>
  <g id="highlight-labels-layer"></g>
  <g id="hitareas-layer">
{}  </g>
  <g id="highlight-hitareas-layer"></g>
"#,
        base_arcs, hitareas
    )
}

fn render_arrow(x: f32, y: f32, class: &str, edge_id: &str) -> String {
    // Arrow pointing left (toward the node at x)
    // Tip at x, base at x+8
    let p1 = format!(
        "{},{}",
        x + LAYOUT.arrow_length,
        y - LAYOUT.arrow_length / 2.0
    ); // top-right
    let p2 = format!("{},{}", x, y); // tip (left, pointing at node)
    let p3 = format!(
        "{},{}",
        x + LAYOUT.arrow_length,
        y + LAYOUT.arrow_length / 2.0
    ); // bottom-right
    format!("    <polygon class=\"{class}\" data-edge=\"{edge_id}\" points=\"{p1} {p2} {p3}\"/>\n")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_empty() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn test_render_single_crate() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "my_crate".into());
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"class="crate""#));
        assert!(svg.contains("my_crate"));
    }

    #[test]
    fn test_render_with_edges() {
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
        ir.add_edge(a, b, EdgeKind::Downward, vec![]);
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(" Q ")); // Bezier
        assert!(svg.contains("<polygon")); // Arrow
    }

    #[test]
    fn test_render_cycle_edges() {
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

        // Test DirectCycle
        ir.add_edge(a, b, EdgeKind::DirectCycle, vec![]);
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("cycle-arc"));
        // DirectCycle should have two arrows (bidirectional)
        // Count polygon elements with cycle-arrow class (not style definition)
        assert_eq!(svg.matches(r#"class="cycle-arrow""#).count(), 2);

        // Test TransitiveCycle
        let mut ir2 = LayoutIR::new();
        let c2 = ir2.add_item(ItemKind::Crate, "c".into());
        let a2 = ir2.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c2,
            },
            "a".into(),
        );
        let b2 = ir2.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c2,
            },
            "b".into(),
        );
        ir2.add_edge(a2, b2, EdgeKind::TransitiveCycle, vec![]);
        let svg2 = render(&ir2, &RenderConfig::default());
        assert!(svg2.contains("cycle-arc"));
        assert!(svg2.contains("stroke-dasharray"));
    }

    #[test]
    fn test_xml_escaping() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "foo<bar>&baz".into());
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("foo&lt;bar&gt;&amp;baz"));
        assert!(!svg.contains("foo<bar>&baz"));
    }

    #[test]
    fn test_tree_lines() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("tree-line"));
    }

    #[test]
    fn test_calculate_text_width() {
        // Monospace 12px → ~7.2px pro Zeichen
        let epsilon = 0.01;
        assert!((calculate_text_width("cli") - 21.6).abs() < epsilon); // 3 * 7.2
        assert!((calculate_text_width("authentication_handler") - 158.4).abs() < epsilon); // 22 * 7.2
    }

    #[test]
    fn test_box_width_from_longest_label() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "cli".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            "authentication_handler".into(),
        );

        let width = calculate_box_width(&ir);
        // längster Name (22 chars) * 7.2 + padding (20px)
        assert!(width >= 158.4 + 20.0);
    }

    #[test]
    fn test_canvas_width_includes_max_arc() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        // 10 Module für große Arc-Distanz
        for i in 0..10 {
            ir.add_item(
                ItemKind::Module {
                    nesting: 1,
                    parent: c,
                },
                format!("m{i}"),
            );
        }
        // Edge von erstem zu letztem Modul (9 Hops)
        ir.add_edge(1, 10, EdgeKind::Downward, vec![]);

        let svg = render(&ir, &RenderConfig::default());
        // Parse viewBox width
        let viewbox_width: f32 = svg
            .lines()
            .find(|l| l.contains("viewBox"))
            .and_then(|l| {
                let start = l.find("viewBox=\"0 0 ")? + 13;
                let rest = &l[start..];
                let end = rest.find(' ')?;
                rest[..end].parse().ok()
            })
            .unwrap();

        // max_arc for 9 hops = 20 + 9*15 + 8 = 163px
        // Canvas must include box_width + max_arc + margin
        let box_width = calculate_box_width(&ir);
        let expected_min = box_width + 163.0;
        assert!(
            viewbox_width >= expected_min,
            "viewBox width {} should be >= {} (box_width {} + arc 163)",
            viewbox_width,
            expected_min,
            box_width
        );
    }

    #[test]
    fn test_all_boxes_same_width() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "short".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            "very_long_module_name".into(),
        );

        let svg = render(&ir, &RenderConfig::default());
        // Extract widths of node rects (crate/module boxes, not toolbar rects)
        let widths: Vec<f32> = svg
            .lines()
            .filter(|l| {
                l.contains("<rect")
                    && (l.contains("class=\"crate\"") || l.contains("class=\"module\""))
            })
            .filter_map(|l| {
                let start = l.find("width=\"")? + 7;
                let rest = &l[start..];
                let end = rest.find('"')?;
                rest[..end].parse().ok()
            })
            .collect();

        assert_eq!(widths.len(), 2, "Expected 2 node rects");
        assert_eq!(
            widths[0], widths[1],
            "All boxes should have same width: {:?}",
            widths
        );
    }

    #[test]
    fn test_nodes_have_ids() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "c".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "m".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"id="node-0""#), "Crate should have id");
        assert!(svg.contains(r#"id="node-1""#), "Module should have id");
    }

    #[test]
    fn test_edges_have_data_attributes() {
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
        ir.add_edge(a, b, EdgeKind::Downward, vec![]);
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"id="edge-1-2""#), "Edge should have id");
        assert!(
            svg.contains(r#"data-from="1""#),
            "Edge should have data-from"
        );
        assert!(svg.contains(r#"data-to="2""#), "Edge should have data-to");
        assert!(
            svg.contains(r#"data-direction="downward""#),
            "Edge should have data-direction"
        );
    }

    #[test]
    fn test_svg_has_script() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("<script>"), "SVG should contain script tag");
        assert!(
            svg.contains("highlightNode"),
            "Script should contain highlightNode function"
        );
        assert!(
            svg.contains("highlightEdge"),
            "Script should contain highlightEdge function"
        );
    }

    #[test]
    fn test_render_has_parent_data_attribute() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent_crate".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child_module".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"data-parent="0""#),
            "Module should have data-parent attribute pointing to crate"
        );
    }

    #[test]
    fn test_render_has_children_attribute() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent_crate".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child_module".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"data-has-children="true""#),
            "Crate with children should have data-has-children attribute"
        );
    }

    #[test]
    fn test_render_tree_lines_have_data_attributes() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        // Tree lines should have data-parent and data-child attributes
        assert!(
            svg.contains(r#"class="tree-line""#) && svg.contains(r#"data-parent="0""#),
            "Tree lines should have data-parent attribute"
        );
        assert!(
            svg.contains(r#"data-child="1""#),
            "Tree lines should have data-child attribute"
        );
    }

    #[test]
    fn test_render_collapse_toggle_present() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"class="collapse-toggle""#),
            "Parent nodes should have collapse toggle"
        );
        assert!(
            svg.contains(r#"data-target="0""#),
            "Collapse toggle should target parent node"
        );
    }

    #[test]
    fn test_render_collapse_css_classes() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(".collapse-toggle"),
            "CSS should contain .collapse-toggle style"
        );
        assert!(
            svg.contains(".collapsed"),
            "CSS should contain .collapsed style"
        );
        assert!(
            svg.contains(".virtual-arc"),
            "CSS should contain .virtual-arc style"
        );
        assert!(
            svg.contains(".arc-count"),
            "CSS should contain .arc-count style"
        );
        assert!(
            svg.contains(".child-count"),
            "CSS should contain .child-count style"
        );
    }

    #[test]
    fn test_render_child_count_tspan() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "child".into(),
        );
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(r#"id="count-0""#),
            "Parent should have child-count tspan with id"
        );
        assert!(
            svg.contains(r#"class="child-count""#),
            "Tspan should have child-count class"
        );
    }

    #[test]
    fn test_render_script_has_collapse_functions() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains("toggleCollapse"),
            "Script should contain toggleCollapse function"
        );
        assert!(
            svg.contains("getDescendants"),
            "Script should contain getDescendants function"
        );
        assert!(
            svg.contains("relayout"),
            "Script should contain relayout function"
        );
        assert!(
            svg.contains("appState"),
            "Script should contain appState for unified state management"
        );
    }

    #[test]
    fn test_render_script_has_hover_functions() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains("AppState.create()"),
            "Script should use AppState module"
        );
        assert!(
            svg.contains("handleMouseEnter"),
            "Script should contain handleMouseEnter function"
        );
        assert!(
            svg.contains("handleMouseLeave"),
            "Script should contain handleMouseLeave function"
        );
        assert!(
            svg.contains("mouseenter"),
            "Script should register mouseenter events"
        );
        assert!(
            svg.contains("mouseleave"),
            "Script should register mouseleave events"
        );
    }

    #[test]
    fn test_render_script_has_toggle_deselect() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        // highlightNode uses AppState.togglePinned for toggle-deselect
        assert!(
            svg.contains("AppState.togglePinned(appState, 'node', nodeId)"),
            "highlightNode should use AppState.togglePinned"
        );
        // highlightEdge uses AppState.togglePinned for toggle-deselect
        assert!(
            svg.contains("AppState.togglePinned(appState, 'edge', edgeId)"),
            "highlightEdge should use AppState.togglePinned"
        );
    }

    #[test]
    fn test_render_edge_source_locations_in_static_data() {
        use crate::graph::SourceLocation;
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
        ir.add_edge(
            a,
            b,
            EdgeKind::Downward,
            vec![SourceLocation {
                file: PathBuf::from("src/a.rs"),
                line: 5,
                symbols: vec![],
            }],
        );
        let svg = render(&ir, &RenderConfig::default());
        // Source locations are now in STATIC_DATA, not DOM attributes
        assert!(
            !svg.contains(r#"data-source-locations="#),
            "DOM attribute should not exist - use STATIC_DATA instead"
        );
        // Location should be in STATIC_DATA usages array
        assert!(svg.contains("src/a.rs:5"));
        assert!(svg.contains("usages: ["));
    }

    #[test]
    fn test_render_has_floating_label_css() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(".floating-label"));
    }

    #[test]
    fn test_render_script_has_floating_label_functions() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("showFloatingLabel"));
        assert!(svg.contains("hideFloatingLabel"));
        assert!(svg.contains("floatingLabel"));
    }

    #[test]
    fn test_render_script_arc_hover_shows_locations() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("dataset.sourceLocations"));
        assert!(svg.contains("showFloatingLabel"));
    }

    #[test]
    fn test_render_script_virtual_arc_aggregates_locations() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        // Virtual arcs should aggregate source locations from hidden edges
        assert!(
            svg.contains("aggregatedLocations") || svg.contains("hiddenEdgeData"),
            "Script should collect locations from hidden edges for virtual arcs"
        );
    }

    #[test]
    fn test_format_source_locations_by_symbol_empty() {
        let locs: Vec<SourceLocation> = vec![];
        assert_eq!(format_source_locations_by_symbol(&locs), "");
    }

    #[test]
    fn test_format_source_locations_by_symbol_no_symbols() {
        use crate::graph::SourceLocation;
        use std::path::PathBuf;

        let locs = vec![SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 7,
            symbols: vec![],
        }];
        assert_eq!(format_source_locations_by_symbol(&locs), "src/cli.rs:7");
    }

    #[test]
    fn test_format_source_locations_by_symbol_single() {
        use crate::graph::SourceLocation;
        use std::path::PathBuf;

        let locs = vec![SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 7,
            symbols: vec!["ModuleInfo".to_string()],
        }];
        // Column-aligned: symbol + padding + arrow + location
        assert_eq!(
            format_source_locations_by_symbol(&locs),
            "ModuleInfo  ← src/cli.rs:7"
        );
    }

    #[test]
    fn test_format_source_locations_by_symbol_grouped() {
        use crate::graph::SourceLocation;
        use std::path::PathBuf;

        // Same symbol from multiple locations
        let locs = vec![
            SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 7,
                symbols: vec!["ModuleInfo".to_string()],
            },
            SourceLocation {
                file: PathBuf::from("src/render.rs"),
                line: 12,
                symbols: vec!["ModuleInfo".to_string()],
            },
        ];
        // Column-aligned: continuation lines have spaces instead of symbol
        assert_eq!(
            format_source_locations_by_symbol(&locs),
            "ModuleInfo  ← src/cli.rs:7|            ← src/render.rs:12"
        );
    }

    #[test]
    fn test_format_source_locations_by_symbol_multiple_symbols() {
        use crate::graph::SourceLocation;
        use std::path::PathBuf;

        // Multiple symbols from same location (multi-import)
        let locs = vec![SourceLocation {
            file: PathBuf::from("src/cli.rs"),
            line: 7,
            symbols: vec!["ModuleInfo".to_string(), "analyze_module".to_string()],
        }];
        // Column-aligned: symbols padded to max length (analyze_module = 14 chars)
        assert_eq!(
            format_source_locations_by_symbol(&locs),
            "ModuleInfo      ← src/cli.rs:7|analyze_module  ← src/cli.rs:7"
        );
    }

    #[test]
    fn test_format_source_locations_by_symbol_complex() {
        use crate::graph::SourceLocation;
        use std::path::PathBuf;

        // Complex case: multiple symbols, multiple locations
        let locs = vec![
            SourceLocation {
                file: PathBuf::from("src/cli.rs"),
                line: 7,
                symbols: vec!["ModuleInfo".to_string(), "analyze_module".to_string()],
            },
            SourceLocation {
                file: PathBuf::from("src/render.rs"),
                line: 12,
                symbols: vec!["ModuleInfo".to_string()],
            },
        ];
        // Column-aligned: ModuleInfo has 2 locations, analyze_module has 1
        assert_eq!(
            format_source_locations_by_symbol(&locs),
            "ModuleInfo      ← src/cli.rs:7|                ← src/render.rs:12|analyze_module  ← src/cli.rs:7"
        );
    }

    #[test]
    fn test_arc_hitarea_css_class_exists() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        // Hit-area CSS class must exist with correct properties
        assert!(
            svg.contains(".arc-hitarea"),
            "CSS should contain .arc-hitarea class"
        );
        assert!(
            svg.contains("pointer-events: stroke"),
            "arc-hitarea should have pointer-events: stroke"
        );
        // Visible arcs should have pointer-events: none
        assert!(
            svg.contains(".dep-arc, .cycle-arc { pointer-events: none; }")
                || svg.contains(".dep-arc, .cycle-arc {") && svg.contains("pointer-events: none"),
            "dep-arc and cycle-arc should have pointer-events: none"
        );
    }

    #[test]
    fn test_arc_has_hitarea_and_visible_path() {
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
        ir.add_edge(a, b, EdgeKind::Downward, vec![]);
        let svg = render(&ir, &RenderConfig::default());

        // Should have two paths for each arc: hit-area and visible
        assert!(
            svg.contains(r#"class="arc-hitarea""#),
            "Should have hit-area path"
        );
        assert!(
            svg.contains(r#"class="dep-arc downward""#),
            "Should have visible dep-arc path with direction"
        );

        // Both should have data-arc-id attribute linking them
        assert!(
            svg.contains(r#"data-arc-id="1-2""#),
            "Both paths should have data-arc-id"
        );

        // Hit-area should have data-from and data-to (source-locations are in STATIC_DATA)
        let hitarea_line = svg
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains("data-arc-id"))
            .expect("Should find hitarea path");
        assert!(
            hitarea_line.contains("data-from="),
            "Hitarea should have data-from"
        );
        assert!(
            hitarea_line.contains("data-to="),
            "Hitarea should have data-to"
        );
    }

    #[test]
    fn test_render_toolbar_contains_elements() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        // Toolbar group
        assert!(
            svg.contains(r#"class="view-options""#),
            "Should have view-options group"
        );
        // Collapse toggle button
        assert!(
            svg.contains(r#"id="collapse-toggle-btn""#),
            "Should have collapse toggle button"
        );
        assert!(
            svg.contains(r#"id="collapse-toggle-label""#),
            "Should have collapse toggle label"
        );
        assert!(
            svg.contains("Collapse All"),
            "Should have 'Collapse All' text"
        );
        // CrateDep checkbox
        assert!(
            svg.contains(r#"id="crate-dep-checkbox""#),
            "Should have crate-dep checkbox"
        );
        assert!(svg.contains("CrateDep"), "Should have 'CrateDep' label");
        // Tests checkbox (disabled)
        assert!(
            svg.contains("toolbar-disabled"),
            "Should have disabled Tests checkbox"
        );
        assert!(svg.contains("Tests"), "Should have 'Tests' label");
    }

    #[test]
    fn test_crate_dep_edges_have_class() {
        let mut ir = LayoutIR::new();
        let c1 = ir.add_item(ItemKind::Crate, "crate_a".into());
        let c2 = ir.add_item(ItemKind::Crate, "crate_b".into());
        ir.add_edge(c1, c2, EdgeKind::Downward, vec![]);
        let svg = render(&ir, &RenderConfig::default());
        // Crate-to-crate edges should have crate-dep-arc class
        assert!(
            svg.contains("crate-dep-arc"),
            "Crate-to-crate edges should have crate-dep-arc class"
        );
    }

    #[test]
    fn test_canvas_includes_toolbar_height() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "test".into());
        let config = RenderConfig::default();
        let svg = render(&ir, &config);

        // Parse viewBox height
        let viewbox_height: f32 = svg
            .lines()
            .find(|l| l.contains("viewBox"))
            .and_then(|l| {
                let start = l.find("viewBox=\"0 0 ")? + 13;
                let rest = &l[start..];
                // Skip width, get height
                let after_width = rest.find(' ')? + 1;
                let height_str = &rest[after_width..];
                let end = height_str.find('"')?;
                height_str[..end].parse().ok()
            })
            .unwrap();

        // Height should include LAYOUT.toolbar.height (40) + margin (20*2) + 1 row (30) + tooltip space
        // Base: margin*2 + rows*row_height + toolbar + tooltip
        assert!(
            viewbox_height >= LAYOUT.toolbar.height + config.margin * 2.0 + config.row_height,
            "Canvas height {} should include toolbar height {}",
            viewbox_height,
            LAYOUT.toolbar.height
        );
    }

    #[test]
    fn test_layer_structure() {
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
        ir.add_edge(a, b, EdgeKind::Downward, vec![]);
        let svg = render(&ir, &RenderConfig::default());

        // Verify all 6 layers exist
        assert!(
            svg.contains(r#"<g id="base-arcs-layer">"#),
            "SVG should contain base-arcs-layer"
        );
        assert!(
            svg.contains(r#"<g id="base-labels-layer">"#),
            "SVG should contain base-labels-layer"
        );
        assert!(
            svg.contains(r#"<g id="highlight-shadows">"#),
            "SVG should contain highlight-shadows layer"
        );
        assert!(
            svg.contains(r#"<g id="highlight-arcs-layer">"#),
            "SVG should contain highlight-arcs-layer"
        );
        assert!(
            svg.contains(r#"<g id="highlight-labels-layer">"#),
            "SVG should contain highlight-labels-layer"
        );
        assert!(
            svg.contains(r#"<g id="hitareas-layer">"#),
            "SVG should contain hitareas-layer"
        );
    }

    #[test]
    fn test_arcs_in_base_arcs_layer() {
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
        ir.add_edge(a, b, EdgeKind::Downward, vec![]);
        let svg = render(&ir, &RenderConfig::default());

        // Find base-arcs-layer content
        let base_arcs_start = svg.find(r#"<g id="base-arcs-layer">"#).unwrap();
        let base_arcs_end = svg[base_arcs_start..].find("</g>").unwrap() + base_arcs_start;
        let base_arcs_content = &svg[base_arcs_start..base_arcs_end];

        // Verify dep-arc is inside base-arcs-layer
        assert!(
            base_arcs_content.contains("dep-arc"),
            "base-arcs-layer should contain dep-arc"
        );
        // Verify arrows are inside base-arcs-layer
        assert!(
            base_arcs_content.contains("<polygon"),
            "base-arcs-layer should contain arrow polygons"
        );
    }

    #[test]
    fn test_hitareas_in_hitareas_layer() {
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
        ir.add_edge(a, b, EdgeKind::Downward, vec![]);
        let svg = render(&ir, &RenderConfig::default());

        // Find hitareas-layer content
        let hitareas_start = svg.find(r#"<g id="hitareas-layer">"#).unwrap();
        let hitareas_end = svg[hitareas_start..].find("</g>").unwrap() + hitareas_start;
        let hitareas_content = &svg[hitareas_start..hitareas_end];

        // Verify arc-hitarea is inside hitareas-layer
        assert!(
            hitareas_content.contains("arc-hitarea"),
            "hitareas-layer should contain arc-hitarea"
        );
    }

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
        use crate::graph::SourceLocation;
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
        ir.add_edge(
            a,
            b,
            EdgeKind::Downward,
            vec![SourceLocation {
                file: PathBuf::from("src/a.rs"),
                line: 5,
                symbols: vec!["MyStruct".to_string()],
            }],
        );

        let config = RenderConfig::default();
        let positioned = calculate_positions(&ir, &config, calculate_box_width(&ir));
        let parents: HashSet<NodeId> = HashSet::from([0]);

        let script = render_script(&config, &ir, &positioned, &parents);

        // Arc should have from, to, usages
        assert!(script.contains(r#""1-2": {"#), "Should have arc 1-2");
        assert!(script.contains(r#"from: "1""#), "Arc should have from");
        assert!(script.contains(r#"to: "2""#), "Arc should have to");
        assert!(script.contains("usages: ["), "Arc should have usages array");
        assert!(script.contains("MyStruct"), "Usages should contain symbol");
        assert!(
            script.contains("src/a.rs:5"),
            "Usages should contain location"
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
        ir.add_edge(a, b, EdgeKind::Downward, vec![]);

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
        use crate::graph::SourceLocation;
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
        ir.add_edge(
            a,
            b,
            EdgeKind::Downward,
            vec![SourceLocation {
                file: PathBuf::from("src/a.rs"),
                line: 5,
                symbols: vec!["Test\"Quote".to_string()],
            }],
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
}
