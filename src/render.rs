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
    pub sidebar: SidebarLayout,
}

/// Sidebar shadow parameters — single source of truth.
/// Used to generate: CSS box-shadow, SVG canvas padding, and JS foreignObject padding.
/// The sidebar sits inside a foreignObject in SVG. Both the foreignObject and the SVG
/// canvas itself clip content at their boundaries. These values ensure the shadow has
/// enough room to render without being cut off.
pub(crate) struct SidebarLayout {
    /// box-shadow Y offset (px)
    pub shadow_offset_y: f32,
    /// box-shadow blur radius (px)
    pub shadow_blur: f32,
    /// box-shadow opacity (0.0–1.0)
    pub shadow_opacity: f32,
}

impl SidebarLayout {
    /// CSS box-shadow value derived from the layout constants.
    pub fn box_shadow_css(&self) -> String {
        format!(
            "0 {}px {}px rgba(0,0,0,{})",
            self.shadow_offset_y as i32, self.shadow_blur as i32, self.shadow_opacity,
        )
    }

    /// Extra padding needed so SVG canvas and foreignObject don't clip the shadow.
    /// max downward extent = offset_y + blur, plus 2px safety margin.
    pub fn shadow_padding(&self) -> f32 {
        self.shadow_offset_y + self.shadow_blur + 2.0
    }
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
    sidebar: SidebarLayout {
        shadow_offset_y: 2.0,
        shadow_blur: 8.0,
        shadow_opacity: 0.12,
    },
};

// --- Color Palette (Catppuccin Latte + Tailwind + Neutrals) ---

const GREEN: &str = "#40a02b";
const YELLOW: &str = "#df8e1d";
const RED: &str = "#d20f39";
const PURPLE: &str = "#8839ef";
const BLUE: &str = "#1e66f5";
const ORANGE: &str = "#fe640b";

const BLUE_100: &str = "#dbeafe";
const BLUE_300: &str = "#93c5fd";
const ORANGE_100: &str = "#ffedd5";
const ORANGE_300: &str = "#fdba74";

const GRAY_600: &str = "#666";
const GRAY_400: &str = "#888";
const GRAY_300: &str = "#ccc";
const GRAY_200: &str = "#e0e0e0";
const GRAY_100: &str = "#f5f5f5";
const GRAY_50: &str = "#fafafa";
const WHITE: &str = "#ffffff";

pub(crate) struct NodeColors {
    pub crate_fill: &'static str,
    pub crate_stroke: &'static str,
    pub module_fill: &'static str,
    pub module_stroke: &'static str,
    pub tree_line: &'static str,
    pub child_count: &'static str,
    pub collapse_toggle: &'static str,
    pub collapse_hover: &'static str,
}

pub(crate) struct DirectionColors {
    pub downward: &'static str,
    pub upward: &'static str,
    pub cycle: &'static str,
    pub count_bg: &'static str,
}

pub(crate) struct NodeSelectionColors {
    pub crate_fill: &'static str,
    pub module_fill: &'static str,
}

pub(crate) struct RelationColors {
    pub dependency: &'static str,
    pub dependent: &'static str,
    pub dimmed: &'static str,
}

pub(crate) struct ToolbarColors {
    pub bg: &'static str,
    pub border: &'static str,
    pub btn_fill: &'static str,
    pub btn_hover: &'static str,
    pub btn_stroke: &'static str,
    pub checkbox: &'static str,
    pub checkbox_checked: &'static str,
    pub separator: &'static str,
}

pub(crate) struct ColorPalette {
    pub nodes: NodeColors,
    pub direction: DirectionColors,
    pub node_selection: NodeSelectionColors,
    pub relation: RelationColors,
    pub toolbar: ToolbarColors,
}

static COLORS: ColorPalette = ColorPalette {
    nodes: NodeColors {
        crate_fill: BLUE_100,
        crate_stroke: BLUE,
        module_fill: ORANGE_100,
        module_stroke: ORANGE,
        tree_line: GRAY_600,
        child_count: GRAY_400,
        collapse_toggle: GRAY_600,
        collapse_hover: BLUE,
    },
    direction: DirectionColors {
        downward: GREEN,
        upward: YELLOW,
        cycle: RED,
        count_bg: WHITE,
    },
    node_selection: NodeSelectionColors {
        crate_fill: BLUE_300,
        module_fill: ORANGE_300,
    },
    relation: RelationColors {
        dependency: GREEN,
        dependent: PURPLE,
        dimmed: GRAY_400,
    },
    toolbar: ToolbarColors {
        bg: GRAY_50,
        border: GRAY_200,
        btn_fill: GRAY_100,
        btn_hover: GRAY_200,
        btn_stroke: GRAY_600,
        checkbox: WHITE,
        checkbox_checked: BLUE,
        separator: GRAY_300,
    },
};

// --- CSS Class Names (Single Source of Truth) ---

#[allow(dead_code)]
pub(crate) struct NodeClasses {
    pub crate_node: &'static str,
    pub module: &'static str,
    pub label: &'static str,
    pub child_count: &'static str,
    pub tree_line: &'static str,
    pub collapse_toggle: &'static str,
    pub collapsed: &'static str,
}

#[allow(dead_code)]
pub(crate) struct DirectionClasses {
    pub dep_arc: &'static str,
    pub downward: &'static str,
    pub upward: &'static str,
    pub dep_arrow: &'static str,
    pub upward_arrow: &'static str,
    pub cycle_arc: &'static str,
    pub cycle_arrow: &'static str,
    pub arc_hitarea: &'static str,
    pub crate_dep_arc: &'static str,
    pub virtual_arc: &'static str,
    pub virtual_arrow: &'static str,
    pub virtual_hitarea: &'static str,
}

#[allow(dead_code)]
pub(crate) struct NodeSelectionClasses {
    pub selected_crate: &'static str,
    pub selected_module: &'static str,
}

#[allow(dead_code)]
pub(crate) struct RelationClasses {
    pub highlighted_arc: &'static str,
    pub highlighted_arrow: &'static str,
    pub highlighted_label: &'static str,
    pub dep_node: &'static str,
    pub dependent_node: &'static str,
    pub dimmed: &'static str,
    pub shadow_path: &'static str,
    pub glow_incoming: &'static str,
    pub glow_outgoing: &'static str,
}

#[allow(dead_code)]
pub(crate) struct ToolbarClasses {
    pub view_options: &'static str,
    pub btn: &'static str,
    pub btn_text: &'static str,
    pub separator: &'static str,
    pub checkbox: &'static str,
    pub checked: &'static str,
    pub disabled: &'static str,
    pub label: &'static str,
}

#[allow(dead_code)]
pub(crate) struct LabelClasses {
    pub arc_count: &'static str,
    pub arc_count_bg: &'static str,
    pub arc_count_group: &'static str,
    pub hidden_by_filter: &'static str,
}

#[allow(dead_code)]
pub(crate) struct SidebarClasses {
    pub root: &'static str,
    pub header: &'static str,
    pub title: &'static str,
    pub close: &'static str,
    pub collapse_all: &'static str,
    pub header_actions: &'static str,
    pub content: &'static str,
    pub usage_group: &'static str,
    pub symbol: &'static str,
    pub location: &'static str,
    pub toggle: &'static str,
    pub symbol_name: &'static str,
    pub ns: &'static str,
    pub ref_count: &'static str,
    pub locations: &'static str,
    pub line_badge: &'static str,
    pub divider: &'static str,
    pub footer: &'static str,
}

#[allow(dead_code)]
pub(crate) struct CssClassNames {
    pub nodes: NodeClasses,
    pub direction: DirectionClasses,
    pub node_selection: NodeSelectionClasses,
    pub relation: RelationClasses,
    pub toolbar: ToolbarClasses,
    pub labels: LabelClasses,
    pub sidebar: SidebarClasses,
}

static CSS: CssClassNames = CssClassNames {
    nodes: NodeClasses {
        crate_node: "crate",
        module: "module",
        label: "label",
        child_count: "child-count",
        tree_line: "tree-line",
        collapse_toggle: "collapse-toggle",
        collapsed: "collapsed",
    },
    direction: DirectionClasses {
        dep_arc: "dep-arc",
        downward: "downward",
        upward: "upward",
        dep_arrow: "dep-arrow",
        upward_arrow: "upward-arrow",
        cycle_arc: "cycle-arc",
        cycle_arrow: "cycle-arrow",
        arc_hitarea: "arc-hitarea",
        crate_dep_arc: "crate-dep-arc",
        virtual_arc: "virtual-arc",
        virtual_arrow: "virtual-arrow",
        virtual_hitarea: "virtual-hitarea",
    },
    node_selection: NodeSelectionClasses {
        selected_crate: "selected-crate",
        selected_module: "selected-module",
    },
    relation: RelationClasses {
        highlighted_arc: "highlighted-arc",
        highlighted_arrow: "highlighted-arrow",
        highlighted_label: "highlighted-label",
        dep_node: "dep-node",
        dependent_node: "dependent-node",
        dimmed: "dimmed",
        shadow_path: "shadow-path",
        glow_incoming: "glow-incoming",
        glow_outgoing: "glow-outgoing",
    },
    toolbar: ToolbarClasses {
        view_options: "view-options",
        btn: "toolbar-btn",
        btn_text: "toolbar-btn-text",
        separator: "toolbar-separator",
        checkbox: "toolbar-checkbox",
        checked: "checked",
        disabled: "toolbar-disabled",
        label: "toolbar-label",
    },
    labels: LabelClasses {
        arc_count: "arc-count",
        arc_count_bg: "arc-count-bg",
        arc_count_group: "arc-count-group",
        hidden_by_filter: "hidden-by-filter",
    },
    sidebar: SidebarClasses {
        root: "sidebar-root",
        header: "sidebar-header",
        title: "sidebar-title",
        close: "sidebar-close",
        collapse_all: "sidebar-collapse-all",
        header_actions: "sidebar-header-actions",
        content: "sidebar-content",
        usage_group: "sidebar-usage-group",
        symbol: "sidebar-symbol",
        location: "sidebar-location",
        toggle: "sidebar-toggle",
        symbol_name: "sidebar-symbol-name",
        ns: "sidebar-ns",
        ref_count: "sidebar-ref-count",
        locations: "sidebar-locations",
        line_badge: "sidebar-line-badge",
        divider: "sidebar-divider",
        footer: "sidebar-footer",
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
    let (width, height) = calculate_canvas_size(&positioned, config, max_arc_width);

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
    svg.push_str(&render_sidebar(width));
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

fn calculate_canvas_size(
    positioned: &[PositionedItem],
    config: &RenderConfig,
    max_arc_width: f32,
) -> (f32, f32) {
    let base_height = if positioned.is_empty() {
        config.margin * 2.0
    } else {
        config.margin * 2.0 + positioned.len() as f32 * config.row_height
    };
    // Sidebar box-shadow extends below the SVG canvas edge when the panel sits
    // near the bottom. The SVG element itself clips anything beyond its viewBox,
    // so we add padding to ensure the shadow renders fully.
    let height = base_height + LAYOUT.toolbar.height + LAYOUT.sidebar.shadow_padding();

    // Width: max(box_right_edge) + arc_space + sidebar_width + margin
    let max_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, |a, b| a.max(b));
    // Use calculated max_arc_width, with a minimum buffer for short/no edges
    let arc_space = max_arc_width.max(LAYOUT.arc_min_space);
    // Reserve space for the sidebar (280px min-width + shadow padding) so it
    // doesn't get clipped when the rightmost arc is selected.
    let sidebar_space = 280.0 + LAYOUT.sidebar.shadow_padding();
    let width = max_x + arc_space + sidebar_space + config.margin;
    (width, height)
}

// --- CSS Builder ---

struct CssRule {
    selector: String,
    properties: Vec<(String, String)>,
}

impl CssRule {
    fn new(selector: &str, properties: &[(&str, &str)]) -> Self {
        Self {
            selector: selector.to_string(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

fn build_css_rules() -> Vec<CssRule> {
    let n = &COLORS.nodes;
    let d = &COLORS.direction;
    let ns = &COLORS.node_selection;
    let r = &COLORS.relation;
    let t = &COLORS.toolbar;
    let c = &CSS;

    vec![
        // Node base styles
        CssRule::new(
            &format!(".{}", c.nodes.crate_node),
            &[
                ("fill", n.crate_fill),
                ("stroke", n.crate_stroke),
                ("stroke-width", "1.5"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.nodes.module),
            &[
                ("fill", n.module_fill),
                ("stroke", n.module_stroke),
                ("stroke-width", "1.5"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.nodes.label),
            &[("font-family", "monospace"), ("font-size", "12px")],
        ),
        CssRule::new(
            &format!(".{}", c.nodes.tree_line),
            &[("stroke", n.tree_line), ("stroke-width", "1")],
        ),
        // Arc base styles
        CssRule::new(
            &format!(".{}, .{}", c.direction.dep_arc, c.direction.cycle_arc),
            &[("pointer-events", "none")],
        ),
        CssRule::new(
            &format!(".{}", c.direction.dep_arc),
            &[("fill", "none"), ("stroke-width", "0.5")],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.dep_arc, c.direction.downward),
            &[("stroke", d.downward)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.dep_arc, c.direction.upward),
            &[("stroke", d.upward)],
        ),
        CssRule::new(
            &format!(".{}", c.direction.dep_arrow),
            &[("fill", d.downward)],
        ),
        CssRule::new(
            &format!(".{}", c.direction.upward_arrow),
            &[("fill", d.upward)],
        ),
        CssRule::new(
            &format!(".{}", c.direction.cycle_arc),
            &[
                ("fill", "none"),
                ("stroke", d.cycle),
                ("stroke-width", "0.5"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.direction.cycle_arrow),
            &[("fill", d.cycle)],
        ),
        // Hit-area
        CssRule::new(
            &format!(".{}", c.direction.arc_hitarea),
            &[
                ("fill", "none"),
                ("stroke", "transparent"),
                ("stroke-width", "12"),
                ("pointer-events", "stroke"),
                ("cursor", "pointer"),
            ],
        ),
        // Selection
        CssRule::new(
            &format!(".{}", c.node_selection.selected_crate),
            &[("fill", ns.crate_fill), ("stroke-width", "3")],
        ),
        CssRule::new(
            &format!(".{}", c.node_selection.selected_module),
            &[("fill", ns.module_fill), ("stroke-width", "3")],
        ),
        // Highlighted arc (marker class)
        CssRule::new(&format!(".{}", c.relation.highlighted_arc), &[]),
        // Glow classes
        CssRule::new(
            &format!(".{}", c.relation.glow_incoming),
            &[("stroke", r.dependency)],
        ),
        CssRule::new(
            &format!(".{}", c.relation.glow_outgoing),
            &[("stroke", r.dependent)],
        ),
        // Node borders (relation)
        CssRule::new(
            &format!(".{}", c.relation.dep_node),
            &[("stroke", r.dependency), ("stroke-width", "2.5")],
        ),
        CssRule::new(
            &format!(".{}", c.relation.dependent_node),
            &[("stroke", r.dependent), ("stroke-width", "2.5")],
        ),
        // Dimmed
        CssRule::new(
            &format!(".{}", c.relation.dimmed),
            &[("opacity", "0.3"), ("pointer-events", "none")],
        ),
        CssRule::new(
            &format!(
                "path.{}:not(.{})",
                c.relation.dimmed, c.relation.shadow_path
            ),
            &[("stroke", r.dimmed)],
        ),
        CssRule::new(
            &format!("polygon.{}", c.relation.dimmed),
            &[("fill", r.dimmed)],
        ),
        // Cursor
        CssRule::new(
            &format!(
                ".{}, .{}, .{}, .{}",
                c.nodes.crate_node, c.nodes.module, c.direction.dep_arc, c.direction.cycle_arc
            ),
            &[("cursor", "pointer")],
        ),
        // Collapse
        CssRule::new(
            &format!(".{}", c.nodes.collapse_toggle),
            &[
                ("font-family", "monospace"),
                ("font-size", "14px"),
                ("cursor", "pointer"),
                ("fill", n.collapse_toggle),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.nodes.collapse_toggle),
            &[("fill", n.collapse_hover)],
        ),
        CssRule::new(&format!(".{}", c.nodes.collapsed), &[("display", "none")]),
        // Virtual arcs
        CssRule::new(
            &format!(".{}", c.direction.virtual_arc),
            &[
                ("fill", "none"),
                ("stroke-width", "0.5"),
                ("stroke-dasharray", "4,2"),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arc, c.direction.downward),
            &[("stroke", d.downward)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arc, c.direction.upward),
            &[("stroke", d.upward)],
        ),
        CssRule::new(
            &format!(".{}", c.direction.virtual_arrow),
            &[("cursor", "pointer")],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arrow, c.direction.downward),
            &[("fill", d.downward)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arrow, c.direction.upward),
            &[("fill", d.upward)],
        ),
        // Arc count labels
        CssRule::new(
            &format!(".{}", c.labels.arc_count),
            &[
                ("font-family", "monospace"),
                ("font-size", "10px"),
                ("fill", d.downward),
                ("text-anchor", "middle"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.labels.arc_count_bg),
            &[("fill", d.count_bg), ("rx", "2")],
        ),
        CssRule::new(
            &format!(".{}.dep-edge", c.labels.arc_count),
            &[
                ("fill", r.dependency),
                ("font-size", "12px"),
                ("font-weight", "bold"),
                ("stroke", "none"),
            ],
        ),
        CssRule::new(
            &format!(".{}.dependent-edge", c.labels.arc_count),
            &[
                ("fill", r.dependent),
                ("font-size", "12px"),
                ("font-weight", "bold"),
                ("stroke", "none"),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.labels.arc_count, c.relation.dimmed),
            &[("opacity", "0.3")],
        ),
        CssRule::new(
            &format!(".{}", c.nodes.child_count),
            &[("font-size", "10px"), ("fill", n.child_count)],
        ),
        // Shadow path
        CssRule::new(
            &format!(".{}", c.relation.shadow_path),
            &[("pointer-events", "none"), ("stroke-linecap", "round")],
        ),
        // Toolbar
        CssRule::new(
            &format!(".{}", c.toolbar.view_options),
            &[("cursor", "default")],
        ),
        CssRule::new(
            &format!(".{}", c.toolbar.btn),
            &[
                ("fill", t.btn_fill),
                ("stroke", t.btn_stroke),
                ("rx", "3"),
                ("cursor", "pointer"),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.toolbar.btn),
            &[("fill", t.btn_hover)],
        ),
        CssRule::new(
            &format!(".{}", c.toolbar.btn_text),
            &[
                ("font-family", "sans-serif"),
                ("font-size", "11px"),
                ("text-anchor", "middle"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.toolbar.checkbox),
            &[
                ("fill", t.checkbox),
                ("stroke", t.btn_stroke),
                ("rx", "2"),
                ("cursor", "pointer"),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.toolbar.checkbox, c.toolbar.checked),
            &[("fill", t.checkbox_checked)],
        ),
        CssRule::new(
            &format!(".{}", c.toolbar.label),
            &[
                ("font-family", "sans-serif"),
                ("font-size", "11px"),
                ("cursor", "pointer"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.toolbar.separator),
            &[("stroke", t.separator)],
        ),
        CssRule::new(
            &format!(".{}", c.toolbar.disabled),
            &[("opacity", "0.4"), ("pointer-events", "none")],
        ),
        // Filter visibility
        CssRule::new(
            &format!(".{}", c.labels.hidden_by_filter),
            &[("display", "none")],
        ),
        // Sidebar
        CssRule::new(
            &format!(".{}", c.sidebar.root),
            &[
                ("background", GRAY_50),
                ("border", &format!("1px solid {}", GRAY_200)),
                ("border-radius", "8px"),
                ("box-shadow", &LAYOUT.sidebar.box_shadow_css()),
                ("font-family", "monospace"),
                ("font-size", "12px"),
                ("color", GRAY_600),
                ("display", "flex"),
                ("flex-direction", "column"),
                ("overflow", "hidden"),
                ("user-select", "text"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.header),
            &[
                ("display", "flex"),
                ("justify-content", "space-between"),
                ("align-items", "center"),
                ("padding", "8px 10px"),
                ("border-bottom", &format!("1px solid {}", GRAY_200)),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.title),
            &[
                ("font-weight", "bold"),
                ("font-size", "13px"),
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "6px"),
            ],
        ),
        CssRule::new(
            ".sidebar-arrow",
            &[
                ("color", GRAY_400),
                ("font-family", "sans-serif"),
                ("font-size", "16px"),
                ("font-weight", "normal"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.close),
            &[
                ("cursor", "pointer"),
                ("font-size", "16px"),
                ("color", GRAY_400),
                ("border", "none"),
                ("background", "none"),
                ("padding", "2px 6px"),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.sidebar.close),
            &[("color", GRAY_600)],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.header_actions),
            &[
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "2px"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.collapse_all),
            &[
                ("cursor", "pointer"),
                ("font-size", "16px"),
                ("color", GRAY_400),
                ("border", "none"),
                ("background", "none"),
                ("padding", "2px 6px"),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.sidebar.collapse_all),
            &[("color", GRAY_600)],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.content),
            &[
                ("overflow-y", "auto"),
                ("padding", "8px 10px"),
                ("flex", "1"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.usage_group),
            &[("margin-bottom", "10px")],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.symbol),
            &[
                ("cursor", "pointer"),
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "4px"),
                ("margin-bottom", "2px"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.location),
            &[
                ("color", GRAY_400),
                ("padding-left", "12px"),
                ("font-size", "11px"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.toggle),
            &[
                ("font-size", "10px"),
                ("color", GRAY_400),
                ("width", "12px"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.symbol_name),
            &[("font-weight", "bold")],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.ns),
            &[("color", GRAY_400), ("font-size", "10px")],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.ref_count),
            &[
                ("color", GRAY_400),
                ("font-size", "10px"),
                ("margin-left", "auto"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.locations),
            &[("padding-left", "16px")],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.line_badge),
            &[
                ("background", BLUE_100),
                ("color", BLUE),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
                ("font-size", "10px"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.divider),
            &[
                ("border", "none"),
                ("border-top", &format!("1px solid {}", GRAY_200)),
                ("margin", "6px 0"),
            ],
        ),
        CssRule::new(
            &format!(".{}", c.sidebar.footer),
            &[
                ("padding", "6px 10px"),
                ("border-top", &format!("1px solid {}", GRAY_200)),
                ("font-size", "10px"),
                ("color", GRAY_400),
            ],
        ),
        CssRule::new(
            ".sidebar-node-crate",
            &[
                ("background", BLUE_100),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::new(
            ".sidebar-node-module",
            &[
                ("background", ORANGE_100),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::new(
            ".sidebar-node-from",
            &[("border", &format!("2px solid {}", PURPLE))],
        ),
        CssRule::new(
            ".sidebar-node-to",
            &[("border", &format!("2px solid {}", GREEN))],
        ),
        CssRule::new(
            ".sidebar-node-crate.sidebar-node-selected",
            &[
                ("background", BLUE_300),
                ("border", &format!("2px solid {}", BLUE)),
            ],
        ),
        CssRule::new(
            ".sidebar-node-module.sidebar-node-selected",
            &[
                ("background", ORANGE_300),
                ("border", &format!("2px solid {}", ORANGE)),
            ],
        ),
        // Transient sidebar mode (hover preview): hide close button and collapse toggles
        CssRule::new(
            &format!(".{}.sidebar-transient .{}", c.sidebar.root, c.sidebar.close),
            &[("display", "none")],
        ),
        CssRule::new(
            &format!(
                ".{}.sidebar-transient .{}",
                c.sidebar.root, c.sidebar.collapse_all
            ),
            &[("display", "none")],
        ),
        CssRule::new(
            &format!(
                ".{}.sidebar-transient .{}",
                c.sidebar.root, c.sidebar.toggle
            ),
            &[
                ("visibility", "hidden"),
                ("width", "0"),
                ("margin", "0"),
                ("padding", "0"),
            ],
        ),
    ]
}

fn render_header(width: f32, height: f32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
"#
    )
}

fn render_styles() -> String {
    let rules = build_css_rules();
    let mut css = String::from("  <style>\n");
    for rule in &rules {
        if rule.properties.is_empty() {
            css.push_str(&format!("    {} {{ }}\n", rule.selector));
        } else {
            css.push_str(&format!("    {} {{ ", rule.selector));
            for (i, (prop, val)) in rule.properties.iter().enumerate() {
                if i > 0 {
                    css.push(' ');
                }
                css.push_str(&format!("{}: {};", prop, val));
            }
            css.push_str(" }\n");
        }
    }
    css.push_str("  </style>\n");
    css
}

fn render_sidebar(width: f32) -> String {
    let x = if width > 280.0 {
        (width - 280.0) as i32
    } else {
        0
    };
    let cs = &CSS.sidebar;
    // overflow:visible lets box-shadow and border-radius render outside the
    // foreignObject boundary (SVG foreignObject defaults to overflow:hidden).
    // Initial height 500 — JS updatePosition() caps dynamically via SIDEBAR_MAX_HEIGHT
    format!(
        concat!(
            "<foreignObject id=\"relation-sidebar\" x=\"{}\" y=\"0\" width=\"280\" height=\"500\" style=\"display:none; overflow:visible\">\n",
            "  <div class=\"{}\" xmlns=\"http://www.w3.org/1999/xhtml\"></div>\n",
            "</foreignObject>\n",
        ),
        x, cs.root,
    )
}

fn render_toolbar(width: f32) -> String {
    let ct = &CSS.toolbar;
    let mut toolbar = String::new();
    let vo = ct.view_options;
    toolbar.push_str(&format!("  <g class=\"{vo}\">\n"));

    // Background rect (full width, toolbar height)
    toolbar.push_str(&format!(
        "    <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"{}\"/>\n",
        width, LAYOUT.toolbar.height, COLORS.toolbar.bg, COLORS.toolbar.border
    ));

    // Collapse/Expand All button
    let tb = &LAYOUT.toolbar;
    let btn_x = tb.btn_x;
    let btn_y = tb.btn_y;
    let btn_width = tb.btn_width;
    let btn_height = tb.btn_height;
    let btn = ct.btn;
    toolbar.push_str(&format!(
        "    <rect id=\"collapse-toggle-btn\" class=\"{btn}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        btn_x, btn_y, btn_width, btn_height
    ));
    let btn_text = ct.btn_text;
    toolbar.push_str(&format!(
        "    <text id=\"collapse-toggle-label\" class=\"{btn_text}\" x=\"{}\" y=\"{}\" dominant-baseline=\"middle\">Collapse All</text>\n",
        btn_x + btn_width / 2.0,
        btn_y + btn_height / 2.0
    ));

    // Separator
    let sep_x = btn_x + btn_width + tb.separator_spacing;
    let sep = ct.separator;
    toolbar.push_str(&format!(
        "    <line class=\"{sep}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
        sep_x, tb.separator_y1, sep_x, tb.separator_y2
    ));

    // CrateDep checkbox (checked by default)
    let cb1_x = sep_x + tb.cb_spacing;
    let cb_y = tb.cb_y;
    let cb_size = tb.cb_size;
    let cb = ct.checkbox;
    let chk = ct.checked;
    let lbl = ct.label;
    toolbar.push_str(&format!(
        "    <rect id=\"crate-dep-checkbox\" class=\"{cb} {chk}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        cb1_x, cb_y, cb_size, cb_size
    ));
    toolbar.push_str(&format!(
        "    <text class=\"{lbl}\" x=\"{}\" y=\"{}\">Show Crate Dependencies</text>\n",
        cb1_x + cb_size + tb.label_x_offset,
        cb_y + cb_size / 2.0 + tb.label_y_offset
    ));

    // Tests checkbox (disabled)
    let cb2_x = cb1_x + tb.cb2_x_offset;
    let dis = ct.disabled;
    toolbar.push_str(&format!(
        "    <rect class=\"{cb} {dis}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        cb2_x, cb_y, cb_size, cb_size
    ));
    toolbar.push_str(&format!(
        "    <text class=\"{lbl} {dis}\" x=\"{}\" y=\"{}\">Tests</text>\n",
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

        let name_escaped = escape_js_string(&item.label);
        lines.push(format!(
            "    \"{}\": {{ type: \"{}\", name: \"{}\", parent: {}, x: {}, y: {}, width: {}, height: {}, hasChildren: {} }}{}",
            pos.id, node_type, name_escaped, parent_str, pos.x, pos.y, pos.width, pos.height, has_children, comma
        ));
    }
    lines.push("  },".to_string());

    // Generate arcs object
    lines.push("  arcs: {".to_string());
    for (i, edge) in ir.edges.iter().enumerate() {
        let arc_id = format!("{}-{}", edge.from, edge.to);

        // Format usages from source_locations as structured objects
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

        let comma = if i < ir.edges.len() - 1 { "," } else { "" };

        lines.push(format!(
            "    \"{}\": {{ from: \"{}\", to: \"{}\", usages: {} }}{}",
            arc_id, edge.from, edge.to, usages_str, comma
        ));
    }
    lines.push("  },".to_string());

    // Generate classes object (CSS class names for JS)
    lines.push("  classes: {".to_string());
    lines.push(format!("    crateNode: \"{}\",", CSS.nodes.crate_node));
    lines.push(format!("    module: \"{}\",", CSS.nodes.module));
    lines.push(format!("    label: \"{}\",", CSS.nodes.label));
    lines.push(format!(
        "    collapseToggle: \"{}\",",
        CSS.nodes.collapse_toggle
    ));
    lines.push(format!("    collapsed: \"{}\",", CSS.nodes.collapsed));
    lines.push(format!("    depArc: \"{}\",", CSS.direction.dep_arc));
    lines.push(format!("    downward: \"{}\",", CSS.direction.downward));
    lines.push(format!("    upward: \"{}\",", CSS.direction.upward));
    lines.push(format!("    depArrow: \"{}\",", CSS.direction.dep_arrow));
    lines.push(format!(
        "    upwardArrow: \"{}\",",
        CSS.direction.upward_arrow
    ));
    lines.push(format!("    cycleArc: \"{}\",", CSS.direction.cycle_arc));
    lines.push(format!(
        "    cycleArrow: \"{}\",",
        CSS.direction.cycle_arrow
    ));
    lines.push(format!(
        "    arcHitarea: \"{}\",",
        CSS.direction.arc_hitarea
    ));
    lines.push(format!(
        "    crateDepArc: \"{}\",",
        CSS.direction.crate_dep_arc
    ));
    lines.push(format!(
        "    virtualArc: \"{}\",",
        CSS.direction.virtual_arc
    ));
    lines.push(format!(
        "    virtualArrow: \"{}\",",
        CSS.direction.virtual_arrow
    ));
    lines.push(format!(
        "    virtualHitarea: \"{}\",",
        CSS.direction.virtual_hitarea
    ));
    lines.push(format!(
        "    selectedCrate: \"{}\",",
        CSS.node_selection.selected_crate
    ));
    lines.push(format!(
        "    selectedModule: \"{}\",",
        CSS.node_selection.selected_module
    ));
    lines.push(format!(
        "    highlightedArc: \"{}\",",
        CSS.relation.highlighted_arc
    ));
    lines.push(format!(
        "    highlightedArrow: \"{}\",",
        CSS.relation.highlighted_arrow
    ));
    lines.push(format!(
        "    highlightedLabel: \"{}\",",
        CSS.relation.highlighted_label
    ));
    lines.push(format!("    depNode: \"{}\",", CSS.relation.dep_node));
    lines.push(format!(
        "    dependentNode: \"{}\",",
        CSS.relation.dependent_node
    ));
    lines.push(format!("    dimmed: \"{}\",", CSS.relation.dimmed));
    lines.push(format!("    shadowPath: \"{}\",", CSS.relation.shadow_path));
    lines.push(format!(
        "    glowIncoming: \"{}\",",
        CSS.relation.glow_incoming
    ));
    lines.push(format!(
        "    glowOutgoing: \"{}\",",
        CSS.relation.glow_outgoing
    ));
    lines.push(format!(
        "    viewOptions: \"{}\",",
        CSS.toolbar.view_options
    ));
    lines.push(format!("    toolbarBtn: \"{}\",", CSS.toolbar.btn));
    lines.push(format!(
        "    toolbarCheckbox: \"{}\",",
        CSS.toolbar.checkbox
    ));
    lines.push(format!("    checked: \"{}\",", CSS.toolbar.checked));
    lines.push(format!("    arcCount: \"{}\",", CSS.labels.arc_count));
    lines.push(format!("    arcCountBg: \"{}\",", CSS.labels.arc_count_bg));
    lines.push(format!(
        "    arcCountGroup: \"{}\",",
        CSS.labels.arc_count_group
    ));
    lines.push(format!(
        "    hiddenByFilter: \"{}\"",
        CSS.labels.hidden_by_filter
    ));
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
                let tl = CSS.nodes.tree_line;

                lines.push_str(&format!(
                    "    <line class=\"{tl}\" x1=\"{line_x}\" y1=\"{parent_bottom}\" x2=\"{line_x}\" y2=\"{child_mid_y}\"{data_attrs}/>\n"
                ));

                let child_left = child_pos.x;
                lines.push_str(&format!(
                    "    <line class=\"{tl}\" x1=\"{line_x}\" y1=\"{child_mid_y}\" x2=\"{child_left}\" y2=\"{child_mid_y}\"{data_attrs}/>\n"
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
            ItemKind::Crate => CSS.nodes.crate_node,
            ItemKind::Module { .. } => CSS.nodes.module,
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
        let lbl = CSS.nodes.label;
        let cc = CSS.nodes.child_count;
        if parents.contains(&item.id) {
            nodes.push_str(&format!(
                "    <text class=\"{lbl}\" x=\"{text_x}\" y=\"{text_y}\">{label}<tspan id=\"count-{}\" class=\"{cc}\"></tspan></text>\n",
                item.id
            ));
        } else {
            nodes.push_str(&format!(
                "    <text class=\"{lbl}\" x=\"{text_x}\" y=\"{text_y}\">{label}</text>\n"
            ));
        }

        // Toggle icon (+/-) for parents
        if parents.contains(&item.id) {
            let toggle_x = item.x + item.width - LAYOUT.toggle_offset;
            let toggle_y = item.y + item.height / 2.0 + LAYOUT.toggle_y_offset;
            let ct = CSS.nodes.collapse_toggle;
            nodes.push_str(&format!(
                "    <text class=\"{ct}\" data-target=\"{}\" x=\"{}\" y=\"{}\">−</text>\n",
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

            let cd = &CSS.direction;
            let (base_arc_class, arrow_class, extra_style, direction) = match edge.kind {
                EdgeKind::Downward => (
                    format!("{} {}", cd.dep_arc, cd.downward),
                    cd.dep_arrow,
                    "",
                    "downward",
                ),
                EdgeKind::Upward => (
                    format!("{} {}", cd.dep_arc, cd.upward),
                    cd.upward_arrow,
                    "",
                    "upward",
                ),
                EdgeKind::DirectCycle => (cd.cycle_arc.to_string(), cd.cycle_arrow, "", "cycle"),
                EdgeKind::TransitiveCycle => (
                    cd.cycle_arc.to_string(),
                    cd.cycle_arrow,
                    " stroke-dasharray=\"4,2\"",
                    "cycle",
                ),
            };

            // Add crate-dep-arc class for Crate-to-Crate edges
            let is_crate_dep = matches!((&from.kind, &to.kind), (ItemKind::Crate, ItemKind::Crate));
            let arc_class = if is_crate_dep {
                format!("{} {}", base_arc_class, cd.crate_dep_arc)
            } else {
                base_arc_class
            };

            let edge_id = format!("{}-{}", edge.from, edge.to);

            // Hit-area path (invisible, 12px wide, receives pointer events) → hitareas layer
            // Note: source-locations are read from STATIC_DATA in JavaScript, not DOM attributes
            let hitarea = cd.arc_hitarea;
            hitareas.push_str(&format!(
                "    <path class=\"{hitarea}\" data-arc-id=\"{edge_id}\" data-from=\"{}\" data-to=\"{}\" data-direction=\"{direction}\" d=\"{path}\"/>\n",
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
    fn test_color_palette_has_expected_values() {
        assert_eq!(COLORS.nodes.crate_fill, "#dbeafe");
        assert_eq!(COLORS.direction.downward, "#40a02b");
        assert_eq!(COLORS.toolbar.bg, "#fafafa");
        assert_eq!(COLORS.node_selection.crate_fill, "#93c5fd");
        assert_eq!(COLORS.relation.dependent, "#8839ef");
    }

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
        // SVG text elements must use XML-escaped entities
        assert!(svg.contains("foo&lt;bar&gt;&amp;baz"));
        // Raw string may appear in CDATA script section (valid), but not in SVG markup.
        // Check that SVG text content (outside CDATA) uses escaped form:
        let outside_cdata = svg.split("CDATA[").next().unwrap_or(&svg);
        assert!(
            !outside_cdata.contains("foo<bar>&baz"),
            "Raw XML-special chars must not appear outside CDATA"
        );
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
                module_path: String::new(),
            }],
        );
        let svg = render(&ir, &RenderConfig::default());
        // Source locations are now in STATIC_DATA, not DOM attributes
        assert!(
            !svg.contains(r#"data-source-locations="#),
            "DOM attribute should not exist - use STATIC_DATA instead"
        );
        // Location should be in STATIC_DATA usages array (structured format)
        assert!(svg.contains(r#"file: "src/a.rs""#));
        assert!(svg.contains("line: 5"));
        assert!(svg.contains("usages: ["));
    }

    #[test]
    fn test_render_script_arc_hover_shows_sidebar() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("showTransient"));
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
        let groups = format_source_locations_by_symbol(&locs);
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_format_source_locations_by_symbol_no_symbols() {
        use crate::graph::SourceLocation;
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
        use crate::graph::SourceLocation;
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
        use crate::graph::SourceLocation;
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
        use crate::graph::SourceLocation;
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
        use crate::graph::SourceLocation;
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

        // Height should include LAYOUT.toolbar.height (40) + margin (20*2) + 1 row (30) + shadow padding
        // Base: margin*2 + rows*row_height + toolbar + shadow_padding
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
                module_path: String::new(),
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
    fn test_static_data_usages_structured() {
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
            vec![
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
            ],
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
                module_path: String::new(),
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

    #[test]
    fn test_css_class_names_not_empty() {
        // Every CSS class name field must be a non-empty string
        assert!(!CSS.nodes.crate_node.is_empty());
        assert!(!CSS.nodes.module.is_empty());
        assert!(!CSS.nodes.label.is_empty());
        assert!(!CSS.nodes.child_count.is_empty());
        assert!(!CSS.nodes.tree_line.is_empty());
        assert!(!CSS.nodes.collapse_toggle.is_empty());
        assert!(!CSS.nodes.collapsed.is_empty());

        assert!(!CSS.direction.dep_arc.is_empty());
        assert!(!CSS.direction.downward.is_empty());
        assert!(!CSS.direction.upward.is_empty());
        assert!(!CSS.direction.dep_arrow.is_empty());
        assert!(!CSS.direction.upward_arrow.is_empty());
        assert!(!CSS.direction.cycle_arc.is_empty());
        assert!(!CSS.direction.cycle_arrow.is_empty());
        assert!(!CSS.direction.arc_hitarea.is_empty());
        assert!(!CSS.direction.crate_dep_arc.is_empty());
        assert!(!CSS.direction.virtual_arc.is_empty());
        assert!(!CSS.direction.virtual_arrow.is_empty());
        assert!(!CSS.direction.virtual_hitarea.is_empty());

        assert!(!CSS.node_selection.selected_crate.is_empty());
        assert!(!CSS.node_selection.selected_module.is_empty());

        assert!(!CSS.relation.highlighted_arc.is_empty());
        assert!(!CSS.relation.highlighted_arrow.is_empty());
        assert!(!CSS.relation.highlighted_label.is_empty());
        assert!(!CSS.relation.dep_node.is_empty());
        assert!(!CSS.relation.dependent_node.is_empty());
        assert!(!CSS.relation.dimmed.is_empty());
        assert!(!CSS.relation.shadow_path.is_empty());
        assert!(!CSS.relation.glow_incoming.is_empty());
        assert!(!CSS.relation.glow_outgoing.is_empty());

        assert!(!CSS.toolbar.view_options.is_empty());
        assert!(!CSS.toolbar.btn.is_empty());
        assert!(!CSS.toolbar.btn_text.is_empty());
        assert!(!CSS.toolbar.separator.is_empty());
        assert!(!CSS.toolbar.checkbox.is_empty());
        assert!(!CSS.toolbar.checked.is_empty());
        assert!(!CSS.toolbar.disabled.is_empty());
        assert!(!CSS.toolbar.label.is_empty());

        assert!(!CSS.labels.arc_count.is_empty());
        assert!(!CSS.labels.arc_count_bg.is_empty());
        assert!(!CSS.labels.arc_count_group.is_empty());
        assert!(!CSS.labels.hidden_by_filter.is_empty());
    }

    #[test]
    fn test_css_builder_parity() {
        // The CSS builder output must match the old format!() output semantically.
        // We verify by checking that all key CSS selectors and properties are present.
        let css = render_styles();

        // Node styles
        assert!(css.contains(&format!(".{}", CSS.nodes.crate_node)));
        assert!(css.contains(&format!(".{}", CSS.nodes.module)));
        assert!(css.contains(&format!(".{}", CSS.nodes.label)));
        assert!(css.contains(&format!(".{}", CSS.nodes.tree_line)));

        // Direction styles
        assert!(css.contains(&format!(".{}", CSS.direction.dep_arc)));
        assert!(css.contains(&format!(".{}", CSS.direction.cycle_arc)));
        assert!(css.contains(&format!(".{}", CSS.direction.dep_arrow)));
        assert!(css.contains(&format!(".{}", CSS.direction.arc_hitarea)));

        // Selection styles
        assert!(css.contains(&format!(".{}", CSS.node_selection.selected_crate)));
        assert!(css.contains(&format!(".{}", CSS.node_selection.selected_module)));

        // Relation styles
        assert!(css.contains(&format!(".{}", CSS.relation.dep_node)));
        assert!(css.contains(&format!(".{}", CSS.relation.dependent_node)));
        assert!(css.contains(&format!(".{}", CSS.relation.dimmed)));
        assert!(css.contains(&format!(".{}", CSS.relation.shadow_path)));

        // Toolbar styles
        assert!(css.contains(&format!(".{}", CSS.toolbar.btn)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.checkbox)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.separator)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.disabled)));

        // Labels
        assert!(css.contains(&format!(".{}", CSS.labels.arc_count)));
        assert!(css.contains(&format!(".{}", CSS.labels.hidden_by_filter)));

        // Color values present
        assert!(css.contains(COLORS.nodes.crate_fill));
        assert!(css.contains(COLORS.direction.downward));
        assert!(css.contains(COLORS.relation.dependency));
        assert!(css.contains(COLORS.toolbar.btn_fill));
    }

    #[test]
    fn test_css_contains_sidebar_rules() {
        let css = render_styles();

        // Sidebar container
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.root)),
            "CSS should contain .sidebar-root"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.header)),
            "CSS should contain .sidebar-header"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.title)),
            "CSS should contain .sidebar-title"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.close)),
            "CSS should contain .sidebar-close"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.content)),
            "CSS should contain .sidebar-content"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.usage_group)),
            "CSS should contain .sidebar-usage-group"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.symbol)),
            "CSS should contain .sidebar-symbol"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.location)),
            "CSS should contain .sidebar-location"
        );

        // Phase 2: 9 neue Sidebar-Klassen
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.toggle)),
            "CSS should contain .sidebar-toggle"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.symbol_name)),
            "CSS should contain .sidebar-symbol-name"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.ns)),
            "CSS should contain .sidebar-ns"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.ref_count)),
            "CSS should contain .sidebar-ref-count"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.locations)),
            "CSS should contain .sidebar-locations"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.line_badge)),
            "CSS should contain .sidebar-line-badge"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.divider)),
            "CSS should contain .sidebar-divider"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.footer)),
            "CSS should contain .sidebar-footer"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.collapse_all)),
            "CSS should contain .sidebar-collapse-all"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.header_actions)),
            "CSS should contain .sidebar-header-actions"
        );
    }

    #[test]
    fn test_render_has_transient_sidebar_css() {
        let css = render_styles();
        assert!(
            css.contains(".sidebar-root.sidebar-transient .sidebar-close"),
            "CSS should contain transient sidebar close rule"
        );
        assert!(
            css.contains("display: none"),
            "Transient sidebar should hide close button"
        );
        assert!(
            css.contains(".sidebar-root.sidebar-transient .sidebar-toggle"),
            "CSS should contain transient sidebar toggle rule"
        );
        assert!(
            css.contains(".sidebar-root.sidebar-transient .sidebar-collapse-all"),
            "CSS should contain transient sidebar collapse-all rule"
        );
    }

    #[test]
    fn test_sidebar_css_properties() {
        let css = render_styles();

        // .sidebar-symbol: cursor:pointer, display:flex
        assert!(css.contains("cursor:pointer") || css.contains("cursor: pointer"));
        assert!(css.contains("display:flex") || css.contains("display: flex"));

        // .sidebar-line-badge: background mit BLUE_100
        assert!(
            css.contains(BLUE_100),
            "CSS should contain BLUE_100 for line-badge background"
        );

        // .sidebar-footer: border-top
        assert!(
            css.contains(".sidebar-footer"),
            "CSS should contain .sidebar-footer selector"
        );
    }

    #[test]
    fn test_css_contains_sidebar_node_selected() {
        let css = render_styles();
        assert!(
            css.contains(".sidebar-node-crate.sidebar-node-selected"),
            "CSS should contain .sidebar-node-crate.sidebar-node-selected"
        );
        assert!(
            css.contains(".sidebar-node-module.sidebar-node-selected"),
            "CSS should contain .sidebar-node-module.sidebar-node-selected"
        );
        // Crate selected: BLUE_300 background + BLUE border
        let crate_rule_idx = css
            .find(".sidebar-node-crate.sidebar-node-selected")
            .unwrap();
        let crate_section = &css[crate_rule_idx..crate_rule_idx + 200];
        assert!(
            crate_section.contains(BLUE_300),
            "Crate selected should use BLUE_300 background"
        );
        assert!(
            crate_section.contains(BLUE),
            "Crate selected should use BLUE border"
        );
        // Module selected: ORANGE_300 background + ORANGE border
        let module_rule_idx = css
            .find(".sidebar-node-module.sidebar-node-selected")
            .unwrap();
        let module_section = &css[module_rule_idx..module_rule_idx + 200];
        assert!(
            module_section.contains(ORANGE_300),
            "Module selected should use ORANGE_300 background"
        );
        assert!(
            module_section.contains(ORANGE),
            "Module selected should use ORANGE border"
        );
    }

    #[test]
    fn test_render_sidebar_basic_structure() {
        let sidebar = render_sidebar(800.0);
        assert!(sidebar.contains("id=\"relation-sidebar\""));
        assert!(sidebar.contains("display:none"));
        assert!(sidebar.contains("width=\"280\""));
        assert!(sidebar.contains(&format!("class=\"{}\"", CSS.sidebar.root)));
        assert!(sidebar.contains("xmlns=\"http://www.w3.org/1999/xhtml\""));
    }

    #[test]
    fn test_render_sidebar_position() {
        let sidebar = render_sidebar(800.0);
        // x should be canvas_width - 280 = 520
        assert!(sidebar.contains("x=\"520\""));

        // Narrow canvas: x should be 0
        let narrow = render_sidebar(200.0);
        assert!(narrow.contains("x=\"0\""));
    }

    #[test]
    fn test_build_css_rules_count() {
        let rules = build_css_rules();
        // We expect a substantial number of CSS rules (roughly 40+)
        assert!(
            rules.len() >= 35,
            "Expected at least 35 CSS rules, got {}",
            rules.len()
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

    #[test]
    fn test_css_rule_selectors_use_constants() {
        let rules = build_css_rules();
        // Verify key selectors reference CSS constants
        let selectors: Vec<&str> = rules.iter().map(|r| r.selector.as_str()).collect();
        assert!(
            selectors.contains(&format!(".{}", CSS.nodes.crate_node).as_str()),
            "Should have .crate rule"
        );
        assert!(
            selectors.contains(&format!(".{}", CSS.nodes.module).as_str()),
            "Should have .module rule"
        );
        assert!(
            selectors.contains(
                &format!(".{}.{}", CSS.direction.dep_arc, CSS.direction.downward).as_str()
            ),
            "Should have .dep-arc.downward rule"
        );
    }

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
}
