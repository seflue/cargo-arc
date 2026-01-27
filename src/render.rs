//! SVG Generation

use crate::graph::SourceLocation;
use crate::layout::{EdgeKind, ItemKind, LayoutIR, NodeId};
use std::collections::HashSet;

/// Character width for monospace 12px font
const CHAR_WIDTH: f32 = 7.2;

/// Calculate text width based on character count
fn calculate_text_width(text: &str) -> f32 {
    text.len() as f32 * CHAR_WIDTH
}

/// Padding for boxes (10px left + 10px right)
const BOX_PADDING: f32 = 20.0;

/// Calculate uniform box width from longest label in LayoutIR
fn calculate_box_width(ir: &LayoutIR) -> f32 {
    ir.items
        .iter()
        .map(|item| calculate_text_width(&item.label))
        .fold(0.0_f32, |a, b| a.max(b))
        + BOX_PADDING
}

/// Arc base offset and scale per hop
const ARC_BASE: f32 = 20.0;
const ARC_SCALE: f32 = 15.0;
const ARROW_SIZE: f32 = 8.0;

/// Padding inside tooltip (matches JavaScript)
const TOOLTIP_PADDING: f32 = 6.0;

/// Offset from cursor to tooltip (matches JavaScript: x + 10)
const TOOLTIP_OFFSET: f32 = 10.0;

/// Separator for tooltip lines (newlines get normalized in XML attributes)
const TOOLTIP_LINE_SEP: &str = "|";

/// Height reserved for the toolbar at the top of the SVG
const TOOLBAR_HEIGHT: f32 = 40.0;

/// Calculate maximum arc width from edges
fn calculate_max_arc_width(positioned: &[PositionedItem], ir: &LayoutIR, row_height: f32) -> f32 {
    ir.edges
        .iter()
        .filter_map(|edge| {
            let from = positioned.iter().find(|p| p.id == edge.from)?;
            let to = positioned.iter().find(|p| p.id == edge.to)?;
            let hops = ((to.y - from.y).abs() / row_height).round().max(1.0);
            Some(ARC_BASE + hops * ARC_SCALE + ARROW_SIZE)
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
    svg.push_str(&render_edges(&positioned, ir));
    svg.push_str(&render_toolbar(width));
    svg.push_str(&render_script(config));
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
                ItemKind::Crate => 24.0,
                ItemKind::Module { .. } => 20.0,
            };
            PositionedItem {
                id: item.id,
                x: config.margin + (nesting as f32 * config.indent_size),
                y: config.margin + TOOLBAR_HEIGHT + (index as f32 * config.row_height),
                width: box_width,
                height,
                label: item.label.clone(),
                kind: item.kind.clone(),
            }
        })
        .collect()
}

/// Line height in tooltip (matches JavaScript)
const TOOLTIP_LINE_HEIGHT: f32 = 14.0;

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
            let height = line_count as f32 * TOOLTIP_LINE_HEIGHT;
            (width, height)
        })
        .fold((0.0_f32, 0.0_f32), |(aw, ah), (w, h)| {
            (aw.max(w), ah.max(h))
        });

    (
        max_width + TOOLTIP_PADDING * 2.0 + TOOLTIP_OFFSET,
        max_height + TOOLTIP_PADDING,
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
    let height = base_height + TOOLBAR_HEIGHT + max_tooltip_height;

    // Width: max(box_right_edge) + arc_space + tooltip_width + margin
    let max_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, |a, b| a.max(b));
    // Use calculated max_arc_width, with a minimum buffer for short/no edges
    let arc_space = max_arc_width.max(50.0);
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

    // Background rect (full width, 40px height)
    toolbar.push_str(&format!(
        "    <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#fafafa\" stroke=\"#e0e0e0\"/>\n",
        width, TOOLBAR_HEIGHT
    ));

    // Collapse/Expand All button (x=10, centered vertically)
    let btn_x = 10.0;
    let btn_y = 8.0;
    let btn_width = 80.0;
    let btn_height = 24.0;
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
    let sep_x = btn_x + btn_width + 15.0;
    toolbar.push_str(&format!(
        "    <line class=\"toolbar-separator\" x1=\"{}\" y1=\"8\" x2=\"{}\" y2=\"32\"/>\n",
        sep_x, sep_x
    ));

    // CrateDep checkbox (checked by default)
    let cb1_x = sep_x + 15.0;
    let cb_y = 12.0;
    let cb_size = 16.0;
    toolbar.push_str(&format!(
        "    <rect id=\"crate-dep-checkbox\" class=\"toolbar-checkbox checked\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        cb1_x, cb_y, cb_size, cb_size
    ));
    toolbar.push_str(&format!(
        "    <text class=\"toolbar-label\" x=\"{}\" y=\"{}\">Show Crate Dependencies</text>\n",
        cb1_x + cb_size + 6.0,
        cb_y + cb_size / 2.0 + 4.0
    ));

    // Tests checkbox (disabled)
    let cb2_x = cb1_x + 190.0;
    toolbar.push_str(&format!(
        "    <rect class=\"toolbar-checkbox toolbar-disabled\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        cb2_x, cb_y, cb_size, cb_size
    ));
    toolbar.push_str(&format!(
        "    <text class=\"toolbar-label toolbar-disabled\" x=\"{}\" y=\"{}\">Tests</text>\n",
        cb2_x + cb_size + 6.0,
        cb_y + cb_size / 2.0 + 4.0
    ));

    toolbar.push_str("  </g>\n");
    toolbar
}

fn render_script(config: &RenderConfig) -> String {
    // Module dependencies must be loaded before svg_script.js
    let tree_logic = include_str!("tree_logic.js");

    let svg_script = include_str!("svg_script.js")
        .replace("__ROW_HEIGHT__", &config.row_height.to_string())
        .replace("__MARGIN__", &config.margin.to_string())
        .replace("__TOOLBAR_HEIGHT__", &TOOLBAR_HEIGHT.to_string());

    format!(
        "  <script><![CDATA[\n{}\n{}\n]]></script>\n",
        tree_logic, svg_script
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
                let line_x = parent_pos.x + 10.0;
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
            ItemKind::Crate => 3.0,
            ItemKind::Module { .. } => 2.0,
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
        let text_x = item.x + 10.0;
        let text_y = item.y + item.height / 2.0 + 4.0;

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
            let toggle_x = item.x + item.width - 14.0;
            let toggle_y = item.y + item.height / 2.0 + 4.0;
            nodes.push_str(&format!(
                "    <text class=\"collapse-toggle\" data-target=\"{}\" x=\"{}\" y=\"{}\">−</text>\n",
                item.id, toggle_x, toggle_y
            ));
        }
    }

    nodes.push_str("  </g>\n");
    nodes
}

fn render_edges(positioned: &[PositionedItem], ir: &LayoutIR) -> String {
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
            let y_offset = 3.0;
            let from_y = from.y + from.height / 2.0 + y_offset; // outgoing: below center
            let to_y = to.y + to.height / 2.0 - y_offset; // incoming: above center

            // Calculate "hops" - how many rows the arc spans
            let row_height = 30.0; // Same as RenderConfig default
            let hops = ((to_y - from_y).abs() / row_height).round().max(1.0);

            // Control point X scales with number of hops
            // Base offset + additional offset per hop
            let arc_offset = 20.0 + (hops * 15.0);
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

            // Build data-source-locations attribute (symbol-grouped format)
            let locations_str = format_source_locations_by_symbol(&edge.source_locations);
            let data_loc_attr = if !locations_str.is_empty() {
                format!(r#" data-source-locations="{}""#, escape_xml(&locations_str))
            } else {
                String::new()
            };

            let edge_id = format!("{}-{}", edge.from, edge.to);

            // Hit-area path (invisible, 12px wide, receives pointer events) → hitareas layer
            hitareas.push_str(&format!(
                "    <path class=\"arc-hitarea\" data-arc-id=\"{edge_id}\" data-from=\"{}\" data-to=\"{}\" data-direction=\"{direction}\" d=\"{path}\"{data_loc_attr}/>\n",
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
    let p1 = format!("{},{}", x + 8.0, y - 4.0); // top-right
    let p2 = format!("{},{}", x, y); // tip (left, pointing at node)
    let p3 = format!("{},{}", x + 8.0, y + 4.0); // bottom-right
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
            svg.contains("collapseState"),
            "Script should contain collapseState map"
        );
    }

    #[test]
    fn test_render_script_has_hover_functions() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains("pinnedHighlight"),
            "Script should contain pinnedHighlight state"
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
        // highlightNode toggle-check
        assert!(
            svg.contains("pinnedHighlight.id === nodeId"),
            "highlightNode should have toggle-check for same node"
        );
        // highlightEdge toggle-check
        assert!(
            svg.contains("pinnedHighlight.id === edgeId"),
            "highlightEdge should have toggle-check for same edge"
        );
    }

    #[test]
    fn test_render_edge_has_data_source_locations() {
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
        assert!(svg.contains(r#"data-source-locations="#));
        assert!(svg.contains("src/a.rs:5"));
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

        // Hit-area should have data-from, data-to, and data-source-locations
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

        // Height should include TOOLBAR_HEIGHT (40) + margin (20*2) + 1 row (30) + tooltip space
        // Base: margin*2 + rows*row_height + toolbar + tooltip
        assert!(
            viewbox_height >= TOOLBAR_HEIGHT + config.margin * 2.0 + config.row_height,
            "Canvas height {} should include toolbar height {}",
            viewbox_height,
            TOOLBAR_HEIGHT
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
}
