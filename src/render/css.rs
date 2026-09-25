use super::constants::{CSS, DRAWING, LAYOUT};
use super::theme::{ColorPalette, LATTE, MOCHA, Mode, THEMES, Theme};
use std::fmt::Write as _;

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

    fn class(name: &str, properties: &[(&str, &str)]) -> Self {
        Self::new(&format!(".{name}"), properties)
    }
}

#[allow(clippy::too_many_lines)] // single cohesive CSS rule list
fn build_css_rules(palette: &ColorPalette) -> Vec<CssRule> {
    let n = &palette.nodes;
    let d = &palette.direction;
    let ns = &palette.node_selection;
    let r = &palette.relation;
    let glow = &palette.glow;
    let tb = &palette.toolbar;
    let sb = &palette.sidebar;
    let pop = &palette.popover;
    let pg = &palette.page;
    let hs = &palette.hotspots;
    let draw = &DRAWING;
    let c = &CSS;

    vec![
        // Node base styles
        CssRule::class(
            c.nodes.crate_node,
            &[
                ("fill", n.crate_fill),
                ("stroke", n.crate_stroke),
                ("stroke-width", "1.5"),
            ],
        ),
        CssRule::class(
            c.nodes.module,
            &[
                ("fill", n.module_fill),
                ("stroke", n.module_stroke),
                ("stroke-width", "1.5"),
            ],
        ),
        CssRule::class(
            c.nodes.external_section,
            &[
                ("fill", n.external_section_fill),
                ("stroke", n.external_section_stroke),
                ("stroke-width", "1.5"),
            ],
        ),
        CssRule::class(
            c.nodes.external_crate,
            &[
                ("fill", n.external_crate_fill),
                ("stroke", n.external_crate_stroke),
                ("stroke-width", "1.5"),
            ],
        ),
        CssRule::class(
            c.nodes.external_transitive,
            &[
                ("fill", n.external_transitive_fill),
                ("stroke", n.external_transitive_stroke),
                ("stroke-width", "1.5"),
            ],
        ),
        CssRule::class(
            c.nodes.label,
            &[
                ("font-family", "monospace"),
                ("font-size", "12px"),
                ("fill", n.text),
                ("pointer-events", "none"),
            ],
        ),
        CssRule::class(
            c.nodes.tree_line,
            &[("stroke", n.tree_line), ("stroke-width", "1")],
        ),
        // Arc base styles
        CssRule::new(
            &format!(".{}, .{}", c.direction.dep_arc, c.direction.cycle_arc),
            &[("pointer-events", "none")],
        ),
        CssRule::class(
            c.direction.dep_arc,
            &[("fill", "none"), ("stroke-width", draw.dep_width)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.dep_arc, c.direction.downward),
            &[("stroke", d.downward)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.dep_arc, c.direction.upward),
            &[("stroke", d.upward)],
        ),
        CssRule::class(c.direction.dep_arrow, &[("fill", d.downward)]),
        CssRule::class(c.direction.upward_arrow, &[("fill", d.upward)]),
        // Cycle color applies only under the container state. Without it, cycle
        // edges keep their directional dep color (they carry dep-arc + direction
        // classes). Placed after the dep rules so the tie in specificity with
        // `.dep-arc.downward` resolves in this rule's favor via source order.
        CssRule::new(
            &format!(".{} .{}", c.relation.cluster_mode_on, c.direction.cycle_arc),
            &[
                ("fill", "none"),
                ("stroke", d.cycle),
                ("stroke-width", draw.cycle_width),
            ],
        ),
        CssRule::new(
            &format!(
                ".{} .{}",
                c.relation.cluster_mode_on, c.direction.cycle_arrow
            ),
            &[("fill", d.cycle)],
        ),
        // Hit-area
        CssRule::class(
            c.direction.arc_hitarea,
            &[
                ("fill", "none"),
                ("stroke", "transparent"),
                ("stroke-width", "12"),
                ("pointer-events", "stroke"),
                ("cursor", "pointer"),
            ],
        ),
        // Selection
        CssRule::class(
            c.node_selection.selected_crate,
            &[("fill", ns.crate_fill), ("stroke-width", "3")],
        ),
        CssRule::class(
            c.node_selection.selected_module,
            &[("fill", ns.module_fill), ("stroke-width", "3")],
        ),
        CssRule::class(
            c.node_selection.selected_external,
            &[("fill", ns.external_fill), ("stroke-width", "3")],
        ),
        CssRule::class(
            c.node_selection.selected_external_transitive,
            &[("fill", ns.external_transitive_fill), ("stroke-width", "3")],
        ),
        CssRule::class(
            c.node_selection.group_member,
            &[("stroke", r.dependency), ("stroke-width", "2")],
        ),
        CssRule::class(
            c.node_selection.cycle_member,
            &[("stroke", d.cycle), ("stroke-width", "1.5")],
        ),
        // Highlighted arc (marker class)
        CssRule::class(c.relation.highlighted_arc, &[]),
        // Glow classes
        CssRule::class(c.relation.glow_incoming, &[("stroke", glow.incoming)]),
        CssRule::class(c.relation.glow_outgoing, &[("stroke", glow.outgoing)]),
        CssRule::class(c.relation.glow_cycle, &[("stroke", glow.cycle)]),
        // Node borders (relation)
        CssRule::class(
            c.relation.dep_node,
            &[
                ("stroke", r.dependency),
                ("stroke-width", draw.relation_border_width),
            ],
        ),
        CssRule::class(
            c.relation.dependent_node,
            &[
                ("stroke", r.dependent),
                ("stroke-width", draw.relation_border_width),
            ],
        ),
        // Dimmed
        CssRule::class(
            c.relation.dimmed,
            &[("opacity", draw.dimmed_opacity), ("pointer-events", "none")],
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
        CssRule::new(
            &format!(
                "polygon.{}.{}",
                c.direction.virtual_arrow, c.relation.dimmed
            ),
            &[("fill", r.dimmed)],
        ),
        // CSS-only dimming via has-highlight on SVG root (leaf elements only).
        // jump-popover is a JS-owned class (js/jump_icons.js), hence the literal.
        CssRule::new(
            &format!(
                "svg.{} rect:not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.jump-popover)",
                c.relation.has_highlight,
                c.node_selection.selected_crate,
                c.node_selection.selected_module,
                c.node_selection.selected_external,
                c.node_selection.selected_external_transitive,
                c.node_selection.group_member,
                c.node_selection.cycle_member,
                c.relation.dep_node,
                c.relation.dependent_node,
                c.toolbar.btn,
                c.labels.arc_count_bg,
            ),
            &[("opacity", draw.dimmed_opacity), ("pointer-events", "none")],
        ),
        // Pinned override: restore pointer-events on dimmed node rects so clicking
        // a different node while one is pinned works (same exclusions as dimming rule)
        CssRule::new(
            &format!(
                "svg.{} rect:not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.jump-popover)",
                c.relation.has_pinned,
                c.node_selection.selected_crate,
                c.node_selection.selected_module,
                c.node_selection.selected_external,
                c.node_selection.selected_external_transitive,
                c.node_selection.group_member,
                c.node_selection.cycle_member,
                c.relation.dep_node,
                c.relation.dependent_node,
                c.toolbar.btn,
                c.labels.arc_count_bg,
            ),
            &[("pointer-events", "auto"), ("cursor", "pointer")],
        ),
        CssRule::new(
            &format!(
                "svg.{} path:not(.{}):not(.{}):not(.{}):not(.{}):not(.{})",
                c.relation.has_highlight,
                c.relation.highlighted_arc,
                c.direction.arc_hitarea,
                c.direction.virtual_hitarea,
                c.relation.shadow_path,
                c.sidebar.cycle_arrow_path
            ),
            &[
                ("opacity", draw.dimmed_opacity),
                ("pointer-events", "none"),
                ("stroke", r.dimmed),
            ],
        ),
        CssRule::new(
            &format!(
                "svg.{} polygon:not(.{})",
                c.relation.has_highlight, c.relation.highlighted_arrow
            ),
            &[
                ("opacity", draw.dimmed_opacity),
                ("pointer-events", "none"),
                ("fill", r.dimmed),
            ],
        ),
        CssRule::new(
            &format!(
                "svg.{} text.{}:not(.{})",
                c.relation.has_highlight, c.labels.arc_count, c.relation.highlighted_label
            ),
            &[("opacity", draw.dimmed_opacity), ("fill", r.dimmed)],
        ),
        CssRule::new(
            &format!("svg.{} line", c.relation.has_highlight),
            &[("opacity", draw.dimmed_opacity), ("pointer-events", "none")],
        ),
        // Toolbar exception: elements inside .view-options never dim
        CssRule::new(
            &format!(
                "svg.{0} .{1} rect, svg.{0} .{1} text, svg.{0} .{1} line",
                c.relation.has_highlight, c.toolbar.view_options
            ),
            &[("opacity", "1"), ("pointer-events", "auto")],
        ),
        // Sidebar exception: elements inside sidebar never dim
        CssRule::new(
            &format!("svg.{} .{} *", c.relation.has_highlight, c.sidebar.root),
            &[("opacity", "1"), ("pointer-events", "auto")],
        ),
        // Cursor
        CssRule::new(
            &format!(
                ".{}, .{}, .{}, .{}, .{}",
                c.nodes.crate_node,
                c.nodes.module,
                c.nodes.external_transitive,
                c.direction.dep_arc,
                c.direction.cycle_arc
            ),
            &[("cursor", "pointer")],
        ),
        // Collapse
        CssRule::class(
            c.nodes.collapse_toggle,
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
        CssRule::class(c.nodes.collapsed, &[("display", "none")]),
        // Re-export arcs (teal, dashed) — republish names without behavioral coupling
        CssRule::class(
            c.direction.reexport_arc,
            &[
                ("fill", "none"),
                ("stroke", d.reexport),
                ("stroke-dasharray", draw.reexport_dash),
            ],
        ),
        // Virtual arcs
        CssRule::class(
            c.direction.virtual_arc,
            &[("fill", "none"), ("stroke-width", draw.virtual_width)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arc, c.direction.downward),
            &[("stroke", d.downward)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arc, c.direction.upward),
            &[("stroke", d.upward)],
        ),
        CssRule::class(c.direction.virtual_arrow, &[("cursor", "pointer")]),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arrow, c.direction.downward),
            &[("fill", d.downward)],
        ),
        CssRule::new(
            &format!(".{}.{}", c.direction.virtual_arrow, c.direction.upward),
            &[("fill", d.upward)],
        ),
        // Arc count labels
        CssRule::class(
            c.labels.arc_count,
            &[
                ("font-family", "monospace"),
                ("font-size", "10px"),
                ("fill", d.downward),
                ("text-anchor", "middle"),
            ],
        ),
        CssRule::class(c.labels.arc_count_bg, &[("fill", d.count_bg), ("rx", "2")]),
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
            &[("opacity", draw.dimmed_opacity), ("fill", r.dimmed)],
        ),
        CssRule::class(
            c.nodes.child_count,
            &[("font-size", "10px"), ("fill", n.child_count)],
        ),
        // Labels on a cycle, and collapsed parents hiding one, share the cycle
        // color; both are gated on the container state like cycle-arc. The
        // marker glyph is always in the DOM (JS fills it on collapse) and only
        // shows under the same state.
        CssRule::new(
            &format!(
                ".{} .{}, .{} .{}",
                c.relation.cluster_mode_on,
                c.nodes.cycle_node,
                c.relation.cluster_mode_on,
                c.nodes.hides_cycle
            ),
            &[("fill", d.cycle)],
        ),
        CssRule::class(c.nodes.cycle_marker, &[("display", "none")]),
        CssRule::new(
            &format!(".{} .{}", c.relation.cluster_mode_on, c.nodes.cycle_marker),
            &[("display", "inline")],
        ),
        // Shadow path
        CssRule::class(
            c.relation.shadow_path,
            &[("pointer-events", "none"), ("stroke-linecap", "round")],
        ),
        // Toolbar (foreignObject HTML)
        CssRule::class(
            c.toolbar.root,
            &[
                ("display", "flex"),
                ("flex-wrap", "wrap"),
                ("align-items", "center"),
                ("gap", "8px"),
                ("padding", "6px 10px"),
                ("width", "100%"),
                ("background", tb.bg),
                ("color", tb.text),
                ("border-bottom", &format!("1px solid {}", tb.border)),
                ("font", "12px/1 system-ui, sans-serif"),
                ("box-sizing", "border-box"),
                ("min-height", "40px"),
            ],
        ),
        CssRule::class(
            c.toolbar.html_btn,
            &[
                ("padding", "4px 12px"),
                ("border", &format!("1px solid {}", tb.control_border)),
                ("border-radius", "3px"),
                ("background", tb.control_bg),
                ("color", tb.text),
                ("cursor", "pointer"),
                ("font-size", "12px"),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.toolbar.html_btn),
            &[("background", tb.btn_hover)],
        ),
        CssRule::class(c.toolbar.dropdown, &[("position", "relative")]),
        CssRule::class(
            c.toolbar.dropdown_panel,
            &[
                ("position", "absolute"),
                ("top", "100%"),
                ("left", "0"),
                ("background", tb.control_bg),
                ("border", &format!("1px solid {}", tb.control_border)),
                ("border-radius", "3px"),
                ("box-shadow", tb.panel_shadow),
                ("padding", "4px 0"),
                ("z-index", "10"),
                ("min-width", "200px"),
            ],
        ),
        CssRule::new(
            &format!(".{} .{}", c.toolbar.dropdown_panel, c.toolbar.toggle),
            &[("padding", "4px 12px")],
        ),
        CssRule::new(
            &format!(".{} .{}:hover", c.toolbar.dropdown_panel, c.toolbar.toggle),
            &[("background", tb.row_hover)],
        ),
        CssRule::class(
            c.toolbar.dropdown_divider,
            &[
                ("border-top", &format!("1px solid {}", tb.border)),
                ("margin", "4px 0"),
            ],
        ),
        CssRule::class(
            c.toolbar.select,
            &[
                ("display", "flex"),
                ("align-items", "center"),
                ("justify-content", "space-between"),
                ("gap", "8px"),
                ("padding", "4px 12px"),
                ("font-size", "12px"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::new(
            &format!(".{} select", c.toolbar.select),
            &[
                ("border", &format!("1px solid {}", tb.control_border)),
                ("border-radius", "3px"),
                ("background", tb.control_bg),
                ("color", tb.text),
                ("font-size", "11px"),
            ],
        ),
        CssRule::class(
            c.toolbar.toggle,
            &[
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "4px"),
                ("cursor", "pointer"),
                ("font-size", "12px"),
                ("user-select", "none"),
            ],
        ),
        CssRule::class(
            c.toolbar.checkbox,
            &[
                ("width", "14px"),
                ("height", "14px"),
                ("border", &format!("1px solid {}", tb.checkbox_border)),
                ("border-radius", "2px"),
                ("display", "inline-flex"),
                ("align-items", "center"),
                ("justify-content", "center"),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}::after", c.toolbar.checkbox, c.toolbar.checked),
            &[
                ("content", r#""\2713""#),
                ("font-size", "11px"),
                ("color", tb.text),
            ],
        ),
        CssRule::class(
            c.toolbar.separator_v,
            &[
                ("width", "1px"),
                ("height", "20px"),
                ("background", tb.control_border),
            ],
        ),
        // Search input group
        CssRule::class(
            c.toolbar.search_group,
            &[
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "6px"),
            ],
        ),
        CssRule::class(c.toolbar.search_input_wrapper, &[("position", "relative")]),
        CssRule::new(
            "#search-input",
            &[
                ("width", "160px"),
                ("padding", "4px 24px 4px 8px"),
                ("border", &format!("1px solid {}", tb.control_border)),
                ("border-radius", "3px"),
                ("background", tb.control_bg),
                ("color", tb.text),
                ("font-size", "12px"),
            ],
        ),
        CssRule::new(
            "#search-input:focus",
            &[("border-color", tb.accent), ("outline", "none")],
        ),
        CssRule::class(
            c.toolbar.search_clear,
            &[
                ("position", "absolute"),
                ("right", "4px"),
                ("top", "50%"),
                ("transform", "translateY(-50%)"),
                ("background", "none"),
                ("border", "none"),
                ("cursor", "pointer"),
                ("font-size", "12px"),
                ("color", tb.text_faint),
            ],
        ),
        // Scope selector (segmented control)
        CssRule::class(
            c.toolbar.scope,
            &[
                ("display", "flex"),
                ("border", &format!("1px solid {}", tb.control_border)),
                ("border-radius", "3px"),
                ("overflow", "hidden"),
            ],
        ),
        CssRule::class(
            c.toolbar.scope_btn,
            &[
                ("padding", "4px 8px"),
                ("border", "none"),
                ("border-right", &format!("1px solid {}", tb.control_border)),
                ("background", tb.control_bg),
                ("color", tb.text),
                ("cursor", "pointer"),
                ("font-size", "11px"),
            ],
        ),
        CssRule::new(
            &format!(".{}:last-child", c.toolbar.scope_btn),
            &[("border-right", "none")],
        ),
        CssRule::new(
            &format!(".{}.{}", c.toolbar.scope_btn, c.toolbar.scope_active),
            &[("background", tb.accent), ("color", tb.on_accent)],
        ),
        CssRule::new(
            &format!(
                ".{}:hover:not(.{})",
                c.toolbar.scope_btn, c.toolbar.scope_active
            ),
            &[("background", tb.row_hover)],
        ),
        CssRule::class(
            c.toolbar.result_count,
            &[
                ("font-size", "11px"),
                ("color", tb.text_muted),
                ("min-width", "60px"),
            ],
        ),
        CssRule::class(
            c.toolbar.jump_status,
            &[("font-size", "11px"), ("color", tb.text_muted)],
        ),
        // Empty flex children still count for .toolbar-root's gap; hide the
        // span so it takes no space while there is no message to show.
        CssRule::new(
            &format!(".{}:empty", c.toolbar.jump_status),
            &[("display", "none")],
        ),
        // The follow and switch toggles carry their state in aria-pressed,
        // written by js/svg_script.js; pressed reads as switched on.
        CssRule::new(
            &format!(
                ".{}[aria-pressed=\"true\"], .{}[aria-pressed=\"true\"]",
                c.toolbar.follow_toggle, c.toolbar.switch_toggle
            ),
            &[
                ("background", tb.pressed_bg),
                ("border-color", tb.pressed_border),
                ("color", tb.pressed_text),
            ],
        ),
        // A switch toggle is busy from the click until the service reports
        // the new page or an error.
        CssRule::new(
            &format!(".{}[aria-busy=\"true\"]", c.toolbar.switch_toggle),
            &[("opacity", "0.6"), ("cursor", "progress")],
        ),
        // CSS-only search dimming via search-active on SVG root
        // Rects: dim all except search matches, toolbar buttons, and arc-count backgrounds
        CssRule::new(
            &format!(
                "svg.{} rect:not(.{}):not(.{}):not(.{}):not(.{})",
                c.search.search_active,
                c.search.search_match,
                c.search.search_match_parent,
                c.toolbar.btn,
                c.labels.arc_count_bg
            ),
            &[("opacity", draw.dimmed_opacity)],
        ),
        // Paths: dim all except search matches, hitareas, and shadow paths
        CssRule::new(
            &format!(
                "svg.{} path:not(.{}):not(.{}):not(.{}):not(.{}):not(.{})",
                c.search.search_active,
                c.search.search_match,
                c.direction.arc_hitarea,
                c.direction.virtual_hitarea,
                c.relation.shadow_path,
                c.sidebar.cycle_arrow_path
            ),
            &[("opacity", draw.dimmed_opacity), ("stroke", r.dimmed)],
        ),
        // Polygons (arrows): dim all except search matches
        CssRule::new(
            &format!(
                "svg.{} polygon:not(.{})",
                c.search.search_active, c.search.search_match
            ),
            &[("opacity", draw.dimmed_opacity), ("fill", r.dimmed)],
        ),
        // Arc count text: dim except search matches
        CssRule::new(
            &format!(
                "svg.{} text.{}:not(.{})",
                c.search.search_active, c.labels.arc_count, c.search.search_match
            ),
            &[("opacity", draw.dimmed_opacity), ("fill", r.dimmed)],
        ),
        // Arc count backgrounds: dim except search matches
        CssRule::new(
            &format!(
                "svg.{} rect.{}:not(.{})",
                c.search.search_active, c.labels.arc_count_bg, c.search.search_match
            ),
            &[("opacity", draw.dimmed_opacity)],
        ),
        // Lines: dim all
        CssRule::new(
            &format!("svg.{} line", c.search.search_active),
            &[("opacity", draw.dimmed_opacity)],
        ),
        // Toolbar exception: elements inside .view-options never dim during search
        CssRule::new(
            &format!(
                "svg.{0} .{1} rect, svg.{0} .{1} text, svg.{0} .{1} line",
                c.search.search_active, c.toolbar.view_options
            ),
            &[("opacity", "1")],
        ),
        // Sidebar exception: elements inside sidebar never dim during search
        CssRule::new(
            &format!("svg.{} .{} *", c.search.search_active, c.sidebar.root),
            &[("opacity", "1")],
        ),
        CssRule::new(
            &format!("rect.{}", c.search.search_match_parent),
            &[
                ("opacity", "0.8"),
                ("stroke", tb.accent),
                ("stroke-width", "2"),
                ("stroke-dasharray", "4 2"),
            ],
        ),
        // Filter visibility
        CssRule::class(c.labels.hidden_by_filter, &[("display", "none")]),
        // Sidebar
        CssRule::class(
            c.sidebar.root,
            &[
                ("background", sb.bg),
                ("border", &format!("1px solid {}", sb.border)),
                ("border-radius", "8px"),
                ("box-shadow", &LAYOUT.sidebar.box_shadow_css()),
                ("font-family", "monospace"),
                ("font-size", "12px"),
                ("color", sb.text),
                ("display", "flex"),
                ("flex-direction", "column"),
                ("overflow", "hidden"),
                ("user-select", "text"),
            ],
        ),
        CssRule::class(
            c.sidebar.header,
            &[
                ("display", "flex"),
                ("justify-content", "space-between"),
                ("align-items", "center"),
                ("padding", "8px 10px"),
                ("border-bottom", &format!("1px solid {}", sb.border)),
            ],
        ),
        CssRule::class(
            c.sidebar.title,
            &[
                ("font-weight", "bold"),
                ("font-size", "13px"),
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "6px"),
            ],
        ),
        // Cluster sidebar counts line ("N modules · M cycles"):
        // literal class string from sidebar.js, no constants.rs entry (Phase 2).
        CssRule::class(
            "sidebar-subheader",
            &[("color", sb.text_muted), ("font-size", "11px")],
        ),
        CssRule::class(
            c.sidebar.arrow,
            &[
                ("color", sb.text_muted),
                ("font-family", "sans-serif"),
                ("font-size", "16px"),
                ("font-weight", "normal"),
            ],
        ),
        CssRule::class(
            c.sidebar.close,
            &[
                ("cursor", "pointer"),
                ("font-size", "16px"),
                ("color", sb.text_muted),
                ("border", "none"),
                ("background", "none"),
                ("padding", "2px 6px"),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.sidebar.close),
            &[("color", sb.text)],
        ),
        CssRule::class(
            c.sidebar.header_actions,
            &[
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "2px"),
            ],
        ),
        CssRule::class(
            c.sidebar.collapse_all,
            &[
                ("cursor", "pointer"),
                ("font-size", "16px"),
                ("color", sb.text_muted),
                ("border", "none"),
                ("background", "none"),
                ("padding", "2px 6px"),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.sidebar.collapse_all),
            &[("color", sb.text)],
        ),
        CssRule::class(
            c.sidebar.content,
            &[
                ("overflow-y", "auto"),
                ("padding", "8px 10px"),
                ("flex", "1"),
                ("min-height", "0"),
            ],
        ),
        CssRule::class(c.sidebar.usage_group, &[("margin-bottom", "10px")]),
        CssRule::class(
            c.sidebar.symbol,
            &[
                ("cursor", "pointer"),
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "4px"),
                ("margin-bottom", "2px"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::class(
            c.sidebar.location,
            &[
                ("color", sb.text_muted),
                ("padding-left", "12px"),
                ("font-size", "11px"),
                ("white-space", "nowrap"),
                ("display", "flex"),
                ("align-items", "center"),
            ],
        ),
        // Rows with data-jump (see js/sidebar.js _locationRow) are click
        // targets only while a selection is pinned, so the cursor and the icon
        // follow the root's has-pinned class.
        CssRule::new(
            &format!(
                "svg.{} .{}[data-jump]",
                c.relation.has_pinned, c.sidebar.location
            ),
            &[("cursor", "pointer")],
        ),
        // Inline <svg class="sidebar-jump"> at the end of a jump-capable row,
        // reusing the #jump-icon symbol; built by js/sidebar.js. Hidden, not
        // removed, until the row is hovered, so the row keeps its width.
        CssRule::class(
            "sidebar-jump",
            &[
                ("width", "12px"),
                ("height", "12px"),
                ("margin-left", "6px"),
                ("vertical-align", "-2px"),
                ("fill", "currentColor"),
                ("visibility", "hidden"),
            ],
        ),
        CssRule::new(
            &format!(
                "svg.{} .{}:hover .sidebar-jump",
                c.relation.has_pinned, c.sidebar.location
            ),
            &[("visibility", "visible")],
        ),
        // The definition chip on a symbol row (js/sidebar.js _definitionChip)
        // follows the location rows: a click target only while pinned, its
        // icon shown only while the row is hovered.
        CssRule::new(
            &format!(
                "svg.{} .sidebar-definition[data-jump]",
                c.relation.has_pinned
            ),
            &[("cursor", "pointer")],
        ),
        CssRule::new(
            &format!(
                "svg.{} .{}:hover .sidebar-jump, svg.{} .sidebar-edge-symbol:hover .sidebar-jump",
                c.relation.has_pinned, c.sidebar.symbol, c.relation.has_pinned
            ),
            &[("visibility", "visible")],
        ),
        // Jump popover: js/jump_icons.js builds it next to a hovered node; no
        // Rust markup emits these classes. The bridge is the transparent strip
        // between node and popover that keeps the pointer inside the group.
        CssRule::class("jump-popover-bridge", &[("fill", "transparent")]),
        CssRule::class(
            "jump-popover-bg",
            &[
                ("fill", pop.bg),
                ("stroke", pop.border),
                ("stroke-width", "1"),
                ("rx", "4px"),
            ],
        ),
        CssRule::class("jump-chip", &[("cursor", "pointer")]),
        CssRule::class(
            "jump-chip-bg",
            &[
                ("fill", pop.chip_bg),
                ("stroke", pop.chip_border),
                ("rx", "3px"),
            ],
        ),
        CssRule::new(
            ".jump-chip:hover .jump-chip-bg",
            &[
                ("fill", pop.chip_hover_bg),
                ("stroke", pop.chip_hover_border),
            ],
        ),
        CssRule::class(
            "jump-chip-label",
            &[
                ("font-family", "monospace"),
                ("font-size", "10px"),
                ("fill", pop.chip_text),
                ("text-anchor", "middle"),
                ("dominant-baseline", "central"),
                ("pointer-events", "none"),
            ],
        ),
        CssRule::class(
            c.sidebar.toggle,
            &[
                ("font-size", "10px"),
                ("color", sb.text_muted),
                ("width", "12px"),
                ("user-select", "none"),
                ("-webkit-user-select", "none"),
            ],
        ),
        CssRule::class(
            c.sidebar.cycle_edge,
            &[
                ("display", "flex"),
                ("align-items", "baseline"),
                ("flex", "1"),
                ("min-width", "0"),
                ("gap", "4px"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::class(
            c.sidebar.cycle_arrow,
            &[("display", "block"), ("margin", "1px 0")],
        ),
        CssRule::new(
            &format!(".{} path", c.sidebar.cycle_arrow),
            &[("stroke", sb.text)],
        ),
        CssRule::class(
            c.sidebar.cycle_node,
            &[
                ("flex", "0 1 auto"),
                ("min-width", "0"),
                ("overflow", "hidden"),
                ("text-overflow", "ellipsis"),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.sidebar.symbol, c.sidebar.symbol_stacked),
            &[("align-items", "flex-start")],
        ),
        // Path spans are the only row children that shrink; js/sidebar.js
        // fitPaths cuts their text on segment boundaries once they overflow,
        // the ellipsis covers whatever that pass leaves over.
        CssRule::class(
            c.sidebar.ns,
            &[
                ("color", sb.text_muted),
                ("font-size", "10px"),
                ("flex", "0 1 auto"),
                ("min-width", "0"),
                ("overflow", "hidden"),
                ("text-overflow", "ellipsis"),
            ],
        ),
        CssRule::class(
            c.sidebar.file,
            &[
                ("flex", "0 1 auto"),
                ("min-width", "0"),
                ("overflow", "hidden"),
                ("text-overflow", "ellipsis"),
            ],
        ),
        CssRule::class(
            c.sidebar.ref_count,
            &[
                ("color", sb.text_muted),
                ("font-size", "10px"),
                ("margin-left", "auto"),
                ("user-select", "none"),
                ("-webkit-user-select", "none"),
            ],
        ),
        // Consumer-locality tag (single-consumer / common-home / crate-wide
        // fact): literal class string from sidebar.js, no constants.rs entry.
        // Neutral pill; the words carry the meaning, a locality-specific colour
        // is deferred.
        CssRule::class(
            "sidebar-locality",
            &[
                ("color", sb.text_muted),
                ("font-size", "9px"),
                ("margin-left", "6px"),
                ("padding", "0 4px"),
                ("border", &format!("1px solid {}", sb.tag_border)),
                ("border-radius", "3px"),
                ("flex-shrink", "0"),
                ("user-select", "none"),
                ("-webkit-user-select", "none"),
            ],
        ),
        CssRule::class(
            c.sidebar.ext_info,
            &[
                ("color", sb.text_muted),
                ("font-size", "8px"),
                ("font-style", "normal"),
                ("margin-left", "auto"),
                ("cursor", "help"),
                ("border", &format!("1px solid {}", sb.tag_border)),
                ("border-radius", "50%"),
                ("width", "12px"),
                ("height", "12px"),
                ("display", "inline-flex"),
                ("align-items", "center"),
                ("justify-content", "center"),
                ("flex-shrink", "0"),
            ],
        ),
        CssRule::class(c.sidebar.locations, &[("padding-left", "16px")]),
        // Cycle-edge row: stacks a one-line edge header over its (hidden until
        // expanded) crossing-symbol list. Literal class strings from
        // sidebar.js, no constants.rs entry.
        CssRule::class(
            "sidebar-edge-row",
            &[("display", "flex"), ("flex-direction", "column")],
        ),
        // Focus row inside an SCC cluster view: marks the edge the graph
        // interaction state machine currently resolves as focus (pin or
        // hover). Literal class string from sidebar.js, no constants.rs entry.
        CssRule::class(
            "sidebar-edge-row-focus",
            &[
                ("background", sb.row_focus_bg),
                ("border-left", &format!("2px solid {}", d.cycle)),
                ("margin-left", "-2px"),
                ("padding-left", "2px"),
            ],
        ),
        CssRule::class(
            "sidebar-edge-meta",
            &[
                ("color", sb.text_muted),
                ("font-size", "10px"),
                ("flex-shrink", "0"),
                ("white-space", "nowrap"),
            ],
        ),
        // Cycle blocks: one collapsible <details> per cycle in a
        // cluster's sidebar section. Literal class strings from
        // sidebar.js, no constants.rs entries (same precedent as the
        // sidebar-edge-row family above).
        CssRule::class("cycle-block", &[("margin-bottom", "8px")]),
        CssRule::class(
            "block-head",
            &[
                ("list-style", "none"),
                ("cursor", "pointer"),
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "6px"),
                ("padding", "2px 0"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::new(
            ".block-head::-webkit-details-marker",
            &[("display", "none")],
        ),
        CssRule::class(
            "block-chevron",
            &[
                ("display", "inline-block"),
                ("flex-shrink", "0"),
                ("width", "12px"),
                ("font-size", "10px"),
                ("color", sb.text_muted),
            ],
        ),
        // Rotates open; transition lives in the reduced-motion media query below.
        CssRule::new(
            ".cycle-block[open] .block-chevron",
            &[("transform", "rotate(90deg)")],
        ),
        CssRule::class(
            "block-ordinal",
            &[
                ("flex-shrink", "0"),
                ("background", sb.ordinal_bg),
                ("color", sb.text),
                ("font-size", "10px"),
                ("padding", "0 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::class(
            "block-path",
            &[
                ("flex", "1 1 auto"),
                ("min-width", "0"),
                ("overflow", "hidden"),
                ("text-overflow", "ellipsis"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::class(
            "block-module-count",
            &[
                ("flex-shrink", "0"),
                ("color", sb.text_muted),
                ("font-size", "10px"),
            ],
        ),
        CssRule::class(
            "cycle-block-body",
            &[("padding-left", "14px"), ("margin-top", "2px")],
        ),
        // Repeated arc: dampened so it visually recedes behind its first
        // occurrence. Closing rule below re-asserts full opacity so a row
        // that were ever both classes at once (JS keeps this from happening)
        // still renders as closing, not repeat.
        CssRule::new(".sidebar-edge-row.edge-repeat", &[("opacity", "0.55")]),
        CssRule::new(".sidebar-edge-row.edge-closing", &[("opacity", "1")]),
        // Closing edge: reuses the existing cycle color (d.cycle, same red as
        // the focus border and glow-cycle) for the arrow plus a leading
        // loop-closer marker, no new color introduced.
        CssRule::new(
            ".sidebar-edge-row.edge-closing .sidebar-arrow",
            &[("color", d.cycle), ("font-weight", "bold")],
        ),
        CssRule::new(
            ".sidebar-edge-closing-marker",
            &[
                ("color", d.cycle),
                ("font-weight", "bold"),
                ("margin-right", "4px"),
            ],
        ),
        // One crossing symbol under an expanded edge row: name + scope tag,
        // indented by the enclosing .sidebar-locations.
        CssRule::class(
            "sidebar-edge-symbol",
            &[
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "4px"),
                ("margin-bottom", "2px"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::class(
            c.sidebar.line_badge,
            &[
                ("background", sb.badge_bg),
                ("color", sb.badge_text),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
                ("font-size", "10px"),
            ],
        ),
        CssRule::class(
            c.sidebar.divider,
            &[
                ("border", "none"),
                ("border-top", &format!("1px solid {}", sb.border)),
                ("margin", "6px 0"),
            ],
        ),
        CssRule::class(
            c.sidebar.footer,
            &[
                ("padding", "6px 10px"),
                ("border-top", &format!("1px solid {}", sb.border)),
                ("font-size", "10px"),
                ("color", sb.text_muted),
            ],
        ),
        // Node badges repeat the node's diagram colours.
        CssRule::class(
            c.sidebar.node_crate,
            &[
                ("background", n.crate_fill),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::class(
            c.sidebar.node_module,
            &[
                ("background", n.module_fill),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::class(
            c.sidebar.node_from,
            &[("border", &format!("2px solid {}", r.dependent))],
        ),
        CssRule::class(
            c.sidebar.node_to,
            &[("border", &format!("2px solid {}", r.dependency))],
        ),
        CssRule::new(
            &format!(".{}.{}", c.sidebar.node_crate, c.sidebar.node_selected),
            &[
                ("background", ns.crate_fill),
                ("border", &format!("2px solid {}", n.crate_stroke)),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.sidebar.node_module, c.sidebar.node_selected),
            &[
                ("background", ns.module_fill),
                ("border", &format!("2px solid {}", n.module_stroke)),
            ],
        ),
        CssRule::class(
            c.sidebar.node_external,
            &[
                ("background", n.external_crate_fill),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::class(
            c.sidebar.node_external_transitive,
            &[
                ("background", n.external_transitive_fill),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::class(
            c.sidebar.node_external_section,
            &[
                ("background", n.external_section_fill),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.sidebar.node_external, c.sidebar.node_selected),
            &[
                ("background", ns.external_fill),
                ("border", &format!("2px solid {}", n.external_crate_stroke)),
            ],
        ),
        CssRule::new(
            &format!(
                ".{}.{}",
                c.sidebar.node_external_transitive, c.sidebar.node_selected
            ),
            &[
                ("background", ns.external_transitive_fill),
                (
                    "border",
                    &format!("2px solid {}", sb.node_external_transitive_selected_border),
                ),
            ],
        ),
        // Badge navigation: clickable node badges
        CssRule::new("[data-node-id]", &[("cursor", "pointer")]),
        // Symbol name badges: inline-flex so min-width works and collapse indicator right-aligns
        CssRule::class(
            c.sidebar.symbol_name,
            &[
                ("display", "inline-flex"),
                ("align-items", "baseline"),
                ("gap", "2px"),
            ],
        ),
        // Collapse indicator (+/−) inside node badges
        CssRule::class(
            c.sidebar.collapse_indicator,
            &[
                ("cursor", "pointer"),
                ("margin-left", "auto"),
                ("font-weight", "bold"),
                ("opacity", "0.6"),
            ],
        ),
        CssRule::new(
            &format!(".{}:hover", c.sidebar.collapse_indicator),
            &[("opacity", "1")],
        ),
        // Transient sidebar mode (hover preview): hide close button and collapse toggles
        CssRule::new(
            &format!(
                ".{}.{} .{}",
                c.sidebar.root, c.sidebar.transient, c.sidebar.close
            ),
            &[("display", "none")],
        ),
        CssRule::new(
            &format!(
                ".{}.{} .{}",
                c.sidebar.root, c.sidebar.transient, c.sidebar.collapse_all
            ),
            &[("display", "none")],
        ),
        CssRule::new(
            &format!(
                ".{}.{} .{}",
                c.sidebar.root, c.sidebar.transient, c.sidebar.toggle
            ),
            &[
                ("visibility", "hidden"),
                ("width", "0"),
                ("margin", "0"),
                ("padding", "0"),
            ],
        ),
        CssRule::new(
            &format!(
                ".{}.{} .{}",
                c.sidebar.root, c.sidebar.transient, c.sidebar.collapse_indicator
            ),
            &[("display", "none")],
        ),
        // Hotspot map: a circle's fill is a per-node color-mix set inline, so
        // the class carries only what every circle shares - a faint hairline
        // in the theme's outline colour, so a container circle is visible
        // before it is hovered or ranked. `g#map-content` scales with the
        // zoom, so a stroke width in user units would grow with it;
        // `non-scaling-stroke` keeps it at the width declared here
        // regardless of zoom.
        // A leaf (a file) paints more opaque than a container (a workspace,
        // crate or module), so the files a hotspot map actually ranks stand
        // out over the containers holding them. The override binds through
        // both classes, so its specificity beats the plain circle rule
        // regardless of which one this sheet declares first.
        CssRule::class(
            c.hotspots.circle,
            &[
                ("stroke", hs.outline),
                ("stroke-opacity", ".28"),
                ("vector-effect", "non-scaling-stroke"),
                ("fill-opacity", "0.32"),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.hotspots.circle, c.hotspots.leaf),
            &[("fill-opacity", "0.92")],
        ),
        // Full stroke-opacity, or the base circle rule's hairline opacity
        // would apply here too and leave the outline no stronger than it.
        CssRule::class(
            c.hotspots.outline,
            &[
                ("stroke", hs.outline),
                ("stroke-width", "3"),
                ("stroke-opacity", "1"),
                ("vector-effect", "non-scaling-stroke"),
            ],
        ),
        CssRule::class(c.hotspots.grey, &[("fill", hs.grey)]),
        // Wider than the outline and the selected ring, so hovering a ranked
        // hotspot still visibly changes it beyond the colour.
        CssRule::class(
            c.hotspots.hover,
            &[
                ("stroke", hs.highlight),
                ("stroke-width", "4"),
                ("stroke-opacity", "1"),
                ("vector-effect", "non-scaling-stroke"),
            ],
        ),
        CssRule::class(
            c.hotspots.selected,
            &[
                ("stroke", hs.highlight),
                ("stroke-width", "3.5"),
                ("stroke-opacity", "1"),
                ("vector-effect", "non-scaling-stroke"),
            ],
        ),
        CssRule::class(
            c.hotspots.label,
            &[
                ("font-family", "monospace"),
                ("font-size", "11px"),
                ("fill", n.text),
                ("text-anchor", "middle"),
                ("pointer-events", "none"),
                ("display", "none"),
                // A halo in the page background colour, so the label stays
                // legible over any fill it sits on. Rounded, or the
                // browser's default mitre join spikes the corners.
                ("paint-order", "stroke"),
                ("stroke", pg.bg),
                ("stroke-width", ".22em"),
                ("stroke-linejoin", "round"),
            ],
        ),
        // The map's own jump icon (js/hotspot_jump_icon.js): no Rust markup
        // emits this class onto an element, only `js/hotspot_jump_icon.js`
        // reads it off the registry. The glyph sits on leaf circles whose
        // fill can be the hottest colour in the ramp, so its halo shares
        // `c.hotspots.label`'s colour, width and join. `stroke-width: .22em`
        // only resolves against a font-size, and the `<use>` element this
        // class sits on carries none of its own, so the rule also sets the
        // label's own font-size to give the em one.
        CssRule::class(
            c.hotspots.jump_icon,
            &[
                ("fill", n.text),
                ("cursor", "pointer"),
                ("font-size", "11px"),
                ("paint-order", "stroke"),
                ("stroke", pg.bg),
                ("stroke-width", ".22em"),
                ("stroke-linejoin", "round"),
            ],
        ),
        // The map's always-visible sidebar content, inside the arc sidebar's
        // own frame (c.sidebar.root already styles the foreignObject around
        // it): the same background/border/text palette, since it is the same
        // panel with different content.
        CssRule::class(
            c.hotspots.sidebar,
            &[
                ("background", sb.bg),
                ("border", &format!("1px solid {}", sb.border)),
                ("border-radius", "8px"),
                ("font-family", "monospace"),
                ("font-size", "12px"),
                ("color", sb.text),
                ("padding", "8px 10px"),
                ("overflow-y", "auto"),
            ],
        ),
        CssRule::class(c.hotspots.details, &[("margin-bottom", "8px")]),
        CssRule::class(
            c.hotspots.details_title,
            &[("font-weight", "bold"), ("margin-bottom", "4px")],
        ),
        CssRule::class(
            c.hotspots.list,
            &[("list-style", "none"), ("margin", "0"), ("padding", "0")],
        ),
        CssRule::class(
            c.hotspots.list_item,
            &[("cursor", "pointer"), ("padding", "2px 0")],
        ),
        CssRule::new(
            &format!(".{}:hover", c.hotspots.list_item),
            &[("background", sb.row_focus_bg)],
        ),
        CssRule::class(
            c.hotspots.list_toggle,
            &[
                ("cursor", "pointer"),
                ("background", tb.control_bg),
                ("border", &format!("1px solid {}", tb.control_border)),
                ("border-radius", "4px"),
                ("color", tb.text),
                ("font-size", "11px"),
                ("padding", "2px 6px"),
                ("margin-bottom", "6px"),
            ],
        ),
        CssRule::class(
            c.hotspots.note,
            &[
                ("color", sb.text_muted),
                ("font-size", "11px"),
                ("margin-top", "6px"),
            ],
        ),
        // A full-width flex item wraps onto its own line of the toolbar.
        CssRule::class(
            c.hotspots.breadcrumb,
            &[
                ("flex-basis", "100%"),
                ("display", "flex"),
                ("align-items", "center"),
                ("gap", "4px"),
                ("height", "24px"),
                ("font-size", "13px"),
                ("white-space", "nowrap"),
                ("overflow", "hidden"),
            ],
        ),
        CssRule::new(
            &format!(".{} button", c.hotspots.breadcrumb),
            &[
                ("background", "none"),
                ("border", "0"),
                ("padding", "0 2px"),
                ("color", tb.accent),
                ("cursor", "pointer"),
                ("font", "inherit"),
            ],
        ),
        CssRule::new(
            &format!(".{} [aria-current]", c.hotspots.breadcrumb),
            &[("color", tb.text), ("font-weight", "600")],
        ),
        CssRule::new(
            &format!(".{} [aria-hidden]", c.hotspots.breadcrumb),
            &[("color", tb.text_muted)],
        ),
        CssRule::class(
            c.hotspots.bar_label,
            &[
                ("display", "block"),
                ("font-size", "11px"),
                ("color", sb.text),
            ],
        ),
        // A visible base for the ranked-bars view: `barFill`'s own inline
        // width/colour draws over it, so the unfilled remainder still reads
        // as a track rather than empty space.
        CssRule::class(
            c.hotspots.bar_track,
            &[
                ("background", sb.border),
                ("border-radius", "3px"),
                ("overflow", "hidden"),
            ],
        ),
        // The hover tooltip (`js/hotspot_hover.js`): a plain `<rect>`/`<text>`
        // pair inside the group, styled by descendant selector the way the
        // jump popover above styles its own unclassed children.
        CssRule::new(
            &format!(".{} rect", c.hotspots.tooltip),
            &[
                ("fill", pop.bg),
                ("stroke", pop.border),
                ("stroke-width", "1"),
            ],
        ),
        CssRule::new(
            &format!(".{} text", c.hotspots.tooltip),
            &[
                ("fill", n.text),
                ("font-family", "monospace"),
                ("font-size", "11px"),
            ],
        ),
    ]
}

/// One block declaring every custom property of `theme` under `selector`.
fn theme_block(selector: &str, theme: &Theme) -> String {
    let mut block = format!("    {selector} {{");
    for (name, value) in theme.palette.variables() {
        let _ = write!(block, " {name}: {value};");
    }
    block.push_str(" }\n");
    block
}

/// The rules read every colour through `var(--arc-…)`; the blocks ahead of
/// them declare the values. `:root` carries the light default and the
/// dark-scheme media query the dark default; `data-mode` on the root picks
/// a mode's default regardless of the system, and `data-theme` a theme by
/// name. The attribute selectors follow the media query in source order
/// and outrank it by specificity, so a pinned root wins.
pub(super) fn render_styles() -> String {
    let rules = build_css_rules(&ColorPalette::VARS);
    let mut css = String::from("  <style>\n");
    css.push_str(&theme_block(":root", &LATTE));
    let _ = writeln!(
        css,
        "    @media (prefers-color-scheme: dark) {{ {}    }}",
        theme_block(":root", &MOCHA).trim_start()
    );
    for mode in [Mode::Light, Mode::Dark] {
        css.push_str(&theme_block(
            &format!(":root[data-mode=\"{}\"]", mode.as_str()),
            Theme::default_for(mode),
        ));
    }
    for theme in THEMES {
        css.push_str(&theme_block(
            &format!(":root[data-theme=\"{}\"]", theme.name),
            theme,
        ));
    }
    for rule in &rules {
        if rule.properties.is_empty() {
            let _ = writeln!(css, "    {} {{ }}", rule.selector);
        } else {
            let _ = write!(css, "    {} {{ ", rule.selector);
            for (i, (prop, val)) in rule.properties.iter().enumerate() {
                if i > 0 {
                    css.push(' ');
                }
                let _ = write!(css, "{prop}: {val};");
            }
            css.push_str(" }\n");
        }
    }
    // Chevron rotation transition, gated behind prefers-reduced-motion so the
    // rotate-on-open (see .cycle-block[open] .block-chevron above) doesn't
    // animate for users who asked to avoid motion.
    css.push_str("    @media (prefers-reduced-motion: no-preference) {\n");
    css.push_str("      .block-chevron { transition: transform 0.15s ease; }\n");
    css.push_str("    }\n");
    css.push_str("  </style>\n");
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stylesheet declares the light theme on `:root`, the dark theme
    /// under the dark-scheme media query, and each theme by name under
    /// `data-theme`, which outranks the media query by specificity.
    #[test]
    fn test_stylesheet_declares_every_theme_as_custom_properties() {
        let css = render_styles();
        let crate_fill = |theme: &Theme| {
            format!(
                "{{ --arc-node-crate-fill: {};",
                theme.palette.nodes.crate_fill
            )
        };
        assert!(
            css.contains(&format!(":root {}", crate_fill(&LATTE))),
            "light theme on :root, got: {css}"
        );
        assert!(
            css.contains(&format!(
                "@media (prefers-color-scheme: dark) {{ :root {}",
                crate_fill(&MOCHA)
            )),
            "dark theme under the media query, got: {css}"
        );
        for theme in &THEMES {
            assert!(
                css.contains(&format!(
                    ":root[data-theme=\"{}\"] {}",
                    theme.name,
                    crate_fill(theme)
                )),
                "{} pinned by data-theme, got: {css}",
                theme.name
            );
        }
        let media_query = css.find("@media (prefers-color-scheme").unwrap();
        let pinned = css.find(":root[data-theme=").unwrap();
        assert!(
            media_query < pinned,
            "a pinned theme must follow the media query"
        );
    }

    /// Rules read their colours through `var(--arc-…)`, so the theme in
    /// force decides them, and every variable a rule names is declared.
    #[test]
    fn test_rules_read_their_colors_through_variables() {
        let css = render_styles();
        assert!(
            css.contains(&format!(
                ".{} {{ fill: var(--arc-node-crate-fill);",
                CSS.nodes.crate_node
            )),
            "got: {css}"
        );
        let declared: std::collections::BTreeSet<&str> =
            LATTE.palette.variables().map(|(name, _)| name).collect();
        let referenced = css.match_indices("var(--arc-").map(|(idx, _)| {
            let start = idx + "var(".len();
            let end = css[start..].find(')').expect("var( is closed") + start;
            &css[start..end]
        });
        for name in referenced {
            assert!(
                declared.contains(name),
                "{name} is referenced but not declared"
            );
        }
    }

    /// Text that used to rely on the browser's default black takes its
    /// colour from the theme, or a dark theme would paint black on dark.
    #[test]
    fn test_text_colors_come_from_the_theme() {
        let css = render_styles();
        let rule_body = |selector: &str| -> String {
            let idx = css
                .find(&format!("{selector} {{"))
                .unwrap_or_else(|| panic!("CSS should contain a rule for {selector}"));
            let end = css[idx..].find('}').map_or(css.len(), |i| idx + i);
            css[idx..end].to_string()
        };
        let vars = &ColorPalette::VARS;
        assert!(
            rule_body(&format!(".{}", CSS.nodes.label))
                .contains(&format!("fill: {}", vars.nodes.text))
        );
        assert!(
            rule_body(&format!(".{}", CSS.toolbar.root))
                .contains(&format!("color: {}", vars.toolbar.text))
        );
        let input = rule_body("#search-input");
        assert!(input.contains(&format!("background: {}", vars.toolbar.control_bg)));
        assert!(input.contains(&format!("color: {}", vars.toolbar.text)));
    }

    /// A rule reads its colour from the palette it was built with, so a
    /// second palette yields a second stylesheet.
    #[test]
    fn test_css_rules_take_their_colors_from_the_given_palette() {
        let mut other = LATTE.palette;
        other.nodes.crate_fill = "#123456";
        other.toolbar.bg = "#654321";
        other.sidebar.text_muted = "#abcdef";
        let css = |palette: &ColorPalette| {
            build_css_rules(palette)
                .iter()
                .flat_map(|rule| rule.properties.iter())
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let default_css = css(&LATTE.palette);
        let other_css = css(&other);
        for color in ["#123456", "#654321", "#abcdef"] {
            assert!(!default_css.contains(color), "{color} in default css");
            assert!(other_css.contains(color), "{color} missing in other css");
        }
    }

    #[test]
    fn test_cycle_color_gated_by_cluster_mode() {
        // The cycle color must only apply under the .cluster-mode-on container
        // state; without it, cycle edges keep the directional dependency color.
        let css = render_styles();
        let d = &CSS.direction;
        let state = CSS.relation.cluster_mode_on;
        let cycle_color = ColorPalette::VARS.direction.cycle;
        // Gated cycle-arc rule carries the cycle stroke color.
        assert!(
            css.contains(&format!(
                ".{state} .{} {{ fill: none; stroke: {cycle_color}",
                d.cycle_arc
            )),
            "cycle-arc color should be gated behind .cluster-mode-on"
        );
        // Gated cycle-arrow rule carries the cycle fill color.
        assert!(
            css.contains(&format!(
                ".{state} .{} {{ fill: {cycle_color}",
                d.cycle_arrow
            )),
            "cycle-arrow color should be gated behind .cluster-mode-on"
        );
    }

    #[test]
    fn test_cycle_label_marks_gated_by_cluster_mode() {
        // Node labels on a cycle, and collapsed parents hiding one, turn red only
        // under the .cluster-mode-on container state; the marker glyph shows
        // only there too.
        let css = render_styles();
        let n = &CSS.nodes;
        let state = CSS.relation.cluster_mode_on;
        let cycle_color = ColorPalette::VARS.direction.cycle;
        assert!(
            css.contains(&format!(
                ".{state} .{}, .{state} .{} {{ fill: {cycle_color}",
                n.cycle_node, n.hides_cycle
            )),
            "cycle label color should be gated behind .cluster-mode-on"
        );
        assert!(
            css.contains(&format!(".{} {{ display: none", n.cycle_marker)),
            "cycle marker should be hidden outside cluster mode"
        );
        assert!(
            css.contains(&format!(".{state} .{} {{ display: inline", n.cycle_marker)),
            "cycle marker should show under .cluster-mode-on"
        );
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
        assert!(css.contains(&format!(".{}", CSS.node_selection.selected_external)));

        // Relation styles
        assert!(css.contains(&format!(".{}", CSS.relation.dep_node)));
        assert!(css.contains(&format!(".{}", CSS.relation.dependent_node)));
        assert!(css.contains(&format!(".{}", CSS.relation.dimmed)));
        assert!(css.contains(&format!(".{}", CSS.relation.shadow_path)));

        // Toolbar styles (HTML foreignObject)
        assert!(css.contains(&format!(".{}", CSS.toolbar.root)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.html_btn)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.checkbox)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.toggle)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.dropdown)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.dropdown_panel)));
        assert!(css.contains(&format!(".{}", CSS.toolbar.scope)));

        // Search highlighting (CSS-only dimming via svg.search-active)
        assert!(css.contains(&format!("svg.{}", CSS.search.search_active)));
        assert!(css.contains(&format!(".{}", CSS.search.search_match_parent)));

        // Labels
        assert!(css.contains(&format!(".{}", CSS.labels.arc_count)));
        assert!(css.contains(&format!(".{}", CSS.labels.hidden_by_filter)));

        // Color values present
        assert!(css.contains(LATTE.palette.nodes.crate_fill));
        assert!(css.contains(LATTE.palette.direction.downward));
        assert!(css.contains(LATTE.palette.relation.dependency));
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
    fn test_css_contains_sidebar_location_jump_rule() {
        let css = render_styles();
        let selector = format!(
            "svg.{} .{}[data-jump]",
            CSS.relation.has_pinned, CSS.sidebar.location
        );
        let idx = css
            .find(&format!("{selector} {{"))
            .unwrap_or_else(|| panic!("CSS should contain a rule for {selector}"));
        let section = &css[idx..idx + 120];
        assert!(
            section.contains("cursor: pointer"),
            "pinned sidebar-location[data-jump] should set cursor: pointer, got: {section}"
        );
        assert!(
            !section.contains("text-decoration"),
            "sidebar-location[data-jump] marks the row with an icon, not an underline, got: {section}"
        );
        assert!(
            !css.contains(&format!("\n.{}[data-jump]", CSS.sidebar.location)),
            "an unpinned row must not look clickable"
        );
    }

    /// The path spans shrink inside their flex row so the sidebar's width
    /// limit cuts the path, not the symbol name or the line badge.
    #[test]
    fn test_css_path_spans_shrink_with_ellipsis() {
        let css = render_styles();
        for class in [CSS.sidebar.ns, CSS.sidebar.file] {
            let idx = css
                .find(&format!(" .{class} {{"))
                .unwrap_or_else(|| panic!("CSS should contain a rule for .{class}"));
            let section = &css[idx..idx + 160];
            for decl in [
                "min-width: 0",
                "overflow: hidden",
                "text-overflow: ellipsis",
            ] {
                assert!(
                    section.contains(decl),
                    ".{class} should set {decl}, got: {section}"
                );
            }
        }
        let idx = css
            .find(&format!(" .{} {{", CSS.sidebar.location))
            .expect("CSS should contain a rule for .sidebar-location");
        let section = &css[idx..idx + 160];
        assert!(
            section.contains("display: flex"),
            ".sidebar-location must be a flex row so its path span can shrink, got: {section}"
        );
    }

    /// The definition chip on a symbol row (js/sidebar.js _definitionChip)
    /// follows the location rows: pointer and icon only while pinned, the
    /// icon only while its row is hovered.
    #[test]
    fn test_css_contains_sidebar_definition_chip_rules() {
        let css = render_styles();
        let rule_body = |selector: &str| -> String {
            let idx = css
                .find(&format!("{selector} {{"))
                .unwrap_or_else(|| panic!("CSS should contain a rule for {selector}"));
            let end = css[idx..].find('}').map_or(css.len(), |i| idx + i);
            css[idx..end].to_string()
        };
        let pinned = rule_body(&format!(
            "svg.{} .sidebar-definition[data-jump]",
            CSS.relation.has_pinned
        ));
        assert!(pinned.contains("cursor: pointer"), "got: {pinned}");
        let hovered = rule_body(&format!(
            "svg.{} .{}:hover .sidebar-jump, svg.{} .sidebar-edge-symbol:hover .sidebar-jump",
            CSS.relation.has_pinned, CSS.sidebar.symbol, CSS.relation.has_pinned
        ));
        assert!(hovered.contains("visibility: visible"), "got: {hovered}");
        assert!(
            !css.contains("\n.sidebar-definition[data-jump]"),
            "an unpinned chip must not look clickable"
        );
    }

    #[test]
    fn test_css_contains_jump_popover_and_sidebar_icon_rules() {
        let css = render_styles();
        let rule_body = |selector: &str| -> String {
            let idx = css
                .find(&format!("{selector} {{"))
                .unwrap_or_else(|| panic!("CSS should contain a rule for {selector}"));
            let end = css[idx..].find('}').map_or(css.len(), |i| idx + i);
            css[idx..end].to_string()
        };
        let bg = rule_body(".jump-popover-bg");
        assert!(
            bg.contains(&format!("fill: {}", ColorPalette::VARS.popover.bg))
                && bg.contains("stroke:")
                && bg.contains("rx:"),
            "popover background should be a filled, outlined, rounded box, got: {bg}"
        );
        let bridge = rule_body(".jump-popover-bridge");
        assert!(
            bridge.contains("fill: transparent"),
            "bridge must catch pointer events without painting, got: {bridge}"
        );
        let chip = rule_body(".jump-chip");
        assert!(
            chip.contains("cursor: pointer"),
            "chips are click targets, got: {chip}"
        );
        let label = rule_body(".jump-chip-label");
        assert!(
            label.contains("text-anchor: middle")
                && label.contains("dominant-baseline: central")
                && label.contains("pointer-events: none"),
            "chip label is centred and lets clicks through to the chip, got: {label}"
        );
        let dimming = format!("svg.{} rect:not(", CSS.relation.has_highlight);
        let dim_idx = css
            .find(&dimming)
            .expect("CSS should contain the has-highlight rect dimming rule");
        let dim_rule = &css[dim_idx..css[dim_idx..].find('{').map_or(css.len(), |i| dim_idx + i)];
        assert!(
            dim_rule.contains(":not(.jump-popover)"),
            "dimming rule must exclude the popover rects, got: {dim_rule}"
        );
        let sidebar_icon = rule_body(".sidebar-jump");
        assert!(
            sidebar_icon.contains("width: 12px")
                && sidebar_icon.contains("height: 12px")
                && sidebar_icon.contains("visibility: hidden"),
            "sidebar-jump should size the inline icon and hide it until hover, got: {sidebar_icon}"
        );
        let hovered = rule_body(&format!(
            "svg.{} .{}:hover .sidebar-jump",
            CSS.relation.has_pinned, CSS.sidebar.location
        ));
        assert!(
            hovered.contains("visibility: visible"),
            "hovering the row must reveal its icon, got: {hovered}"
        );
    }

    #[test]
    fn test_css_marks_the_pressed_follow_toggle() {
        let css = render_styles();

        assert!(
            css.contains(&format!(
                ".{}[aria-pressed=\"true\"]",
                CSS.toolbar.follow_toggle
            )),
            "CSS should style the pressed follow toggle apart from the released one"
        );
    }

    /// A switch toggle reads like the follow toggle when pressed, and
    /// shows that a run is under way while `aria-busy` is set.
    #[test]
    fn test_css_marks_the_pressed_and_the_busy_switch_toggle() {
        let css = render_styles();

        assert!(
            css.contains(&format!(
                ".{}[aria-pressed=\"true\"]",
                CSS.toolbar.switch_toggle
            )),
            "{css}"
        );
        assert!(
            css.contains(&format!(
                ".{}[aria-busy=\"true\"]",
                CSS.toolbar.switch_toggle
            )),
            "{css}"
        );
    }

    #[test]
    fn test_css_contains_jump_status_rule() {
        let css = render_styles();

        assert!(
            css.contains(&format!(".{}", CSS.toolbar.jump_status)),
            "CSS should contain a rule for .toolbar-jump-status"
        );
    }

    #[test]
    fn test_css_hides_empty_jump_status_span() {
        let css = render_styles();

        assert!(
            css.contains(&format!(".{}:empty", CSS.toolbar.jump_status)),
            "an empty jump-status span should take no space in the flex toolbar"
        );
    }

    #[test]
    fn test_css_contains_focus_cycle_glow_rule() {
        let css = render_styles();

        // The focused cycle edge (marked highlighted-arc + glow-cycle by
        // DerivedState) gets a backing glow in the cycle glow color, same
        // style as glow-incoming/glow-outgoing.
        assert!(
            css.contains(&format!(
                ".{} {{ stroke: {};",
                CSS.relation.glow_cycle,
                ColorPalette::VARS.glow.cycle
            )),
            "CSS should contain .glow-cycle styled in the cycle glow color"
        );
    }

    #[test]
    fn test_css_contains_sidebar_meta_rules() {
        let css = render_styles();

        assert!(
            css.contains(".sidebar-subheader"),
            "CSS should contain .sidebar-subheader"
        );
        assert!(
            css.contains(".sidebar-edge-meta"),
            "CSS should contain .sidebar-edge-meta"
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
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.cycle_edge)),
            "CSS should contain .sidebar-cycle-edge"
        );
        assert!(
            css.contains(&format!(".{}", CSS.sidebar.cycle_arrow)),
            "CSS should contain .sidebar-cycle-arrow"
        );
    }

    #[test]
    fn test_sidebar_css_properties() {
        let css = render_styles();

        // .sidebar-symbol: cursor:pointer, display:flex
        assert!(css.contains("cursor:pointer") || css.contains("cursor: pointer"));
        assert!(css.contains("display:flex") || css.contains("display: flex"));

        // .sidebar-line-badge: background from the sidebar palette
        assert!(
            css.contains(ColorPalette::VARS.sidebar.badge_bg),
            "CSS should contain the badge background for line-badge"
        );

        // .sidebar-footer: border-top
        assert!(
            css.contains(".sidebar-footer"),
            "CSS should contain .sidebar-footer selector"
        );

        // External badge rules
        assert!(
            css.contains(".sidebar-node-external"),
            "CSS should contain .sidebar-node-external"
        );
        assert!(
            css.contains(".sidebar-node-external-transitive"),
            "CSS should contain .sidebar-node-external-transitive"
        );
        assert!(
            css.contains(".sidebar-node-external-section"),
            "CSS should contain .sidebar-node-external-section"
        );

        // External selected-state rules
        assert!(
            css.contains(".sidebar-node-external.sidebar-node-selected"),
            "CSS should contain .sidebar-node-external.sidebar-node-selected"
        );
        assert!(
            css.contains(".sidebar-node-external-transitive.sidebar-node-selected"),
            "CSS should contain .sidebar-node-external-transitive.sidebar-node-selected"
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
        // Crate selected: selection fill + crate stroke as border
        let crate_rule_idx = css
            .find(".sidebar-node-crate.sidebar-node-selected")
            .unwrap();
        let crate_section = &css[crate_rule_idx..crate_rule_idx + 200];
        assert!(
            crate_section.contains(ColorPalette::VARS.node_selection.crate_fill),
            "Crate selected should use the crate selection fill"
        );
        assert!(
            crate_section.contains(ColorPalette::VARS.nodes.crate_stroke),
            "Crate selected should use the crate stroke as border"
        );
        // Module selected: selection fill + module stroke as border
        let module_rule_idx = css
            .find(".sidebar-node-module.sidebar-node-selected")
            .unwrap();
        let module_section = &css[module_rule_idx..module_rule_idx + 200];
        assert!(
            module_section.contains(ColorPalette::VARS.node_selection.module_fill),
            "Module selected should use the module selection fill"
        );
        assert!(
            module_section.contains(ColorPalette::VARS.nodes.module_stroke),
            "Module selected should use the module stroke as border"
        );
    }

    #[test]
    fn test_css_contains_sidebar_external_node_selected() {
        let css = render_styles();

        // External selected: selection fill + external crate stroke as border
        let ext_rule_idx = css
            .find(".sidebar-node-external.sidebar-node-selected")
            .unwrap();
        let ext_section = &css[ext_rule_idx..ext_rule_idx + 200];
        assert!(
            ext_section.contains(ColorPalette::VARS.node_selection.external_fill),
            "External selected should use the external selection fill"
        );
        assert!(
            ext_section.contains(ColorPalette::VARS.nodes.external_crate_stroke),
            "External selected should use the external crate stroke as border"
        );

        // External-transitive selected: selection fill + its own border
        let ext_t_rule_idx = css
            .find(".sidebar-node-external-transitive.sidebar-node-selected")
            .unwrap();
        let ext_t_section = &css[ext_t_rule_idx..ext_t_rule_idx + 200];
        assert!(
            ext_t_section.contains(ColorPalette::VARS.node_selection.external_transitive_fill),
            "External-transitive selected should use the transitive selection fill"
        );
        assert!(
            ext_t_section.contains(
                ColorPalette::VARS
                    .sidebar
                    .node_external_transitive_selected_border
            ),
            "External-transitive selected should use the sidebar border colour"
        );
    }

    #[test]
    fn test_css_contains_badge_navigation_cursor() {
        let css = render_styles();
        let idx = css
            .find("[data-node-id]")
            .expect("CSS should contain [data-node-id] selector");
        let section = &css[idx..idx + 80];
        assert!(
            section.contains("cursor: pointer"),
            "Badge navigation rule should set cursor: pointer, got: {section}"
        );
    }

    #[test]
    fn test_css_has_pinned_override_rule() {
        let css = render_styles();
        let pinned_start = css
            .find(&format!("svg.{} rect:not(", CSS.relation.has_pinned))
            .expect("has-pinned override rule should exist");
        let pinned_section = &css[pinned_start..pinned_start + 400];
        assert!(
            pinned_section.contains("pointer-events: auto"),
            "has-pinned rule should restore pointer-events, got: {pinned_section}"
        );
        assert!(
            pinned_section.contains("cursor: pointer"),
            "has-pinned rule should set cursor: pointer, got: {pinned_section}"
        );
    }

    #[test]
    fn test_css_group_member_excluded_from_dimming() {
        let css = render_styles();
        // The rect dimming rule should exclude .group-member so group members are not dimmed
        // Find the rect dimming rule (svg.has-highlight rect:not(...))
        let rect_dim_start = css
            .find("svg.has-highlight rect:not(")
            .expect("rect dimming rule should exist");
        let rect_dim_section = &css[rect_dim_start..rect_dim_start + 300];
        assert!(
            rect_dim_section.contains(&format!(":not(.{})", CSS.node_selection.group_member)),
            "rect dimming rule should exclude .group-member, got: {rect_dim_section}"
        );
    }

    #[test]
    fn test_css_contains_cycle_member() {
        let css = render_styles();
        assert!(
            css.contains(&format!(".{}", CSS.node_selection.cycle_member)),
            "CSS should contain .cycle-member class"
        );
        // cycle-member should use the cycle color for stroke
        assert!(
            css.contains(&format!(
                ".{} {{ stroke: {};",
                CSS.node_selection.cycle_member,
                ColorPalette::VARS.direction.cycle
            )),
            "cycle-member should use cycle color for stroke"
        );
    }

    #[test]
    fn test_css_contains_sidebar_collapse_indicator() {
        let css = render_styles();
        let cls = CSS.sidebar.collapse_indicator;
        assert!(
            css.contains(&format!(".{cls}")),
            "CSS should contain .sidebar-collapse-indicator"
        );
        // Base rule: cursor pointer, opacity
        let base_idx = css
            .find(&format!(".{cls} {{"))
            .expect(".sidebar-collapse-indicator base rule should exist");
        let base_section = &css[base_idx..base_idx + 150];
        assert!(
            base_section.contains("cursor: pointer"),
            "collapse-indicator should have cursor: pointer, got: {base_section}"
        );
        assert!(
            base_section.contains("opacity: 0.6"),
            "collapse-indicator should have opacity: 0.6, got: {base_section}"
        );
        // Hover rule
        assert!(
            css.contains(&format!(".{cls}:hover")),
            "CSS should contain .sidebar-collapse-indicator:hover"
        );
        // Transient mode: hidden
        assert!(
            css.contains(&format!(
                ".{}.{} .{cls}",
                CSS.sidebar.root, CSS.sidebar.transient
            )),
            "CSS should hide collapse-indicator in transient sidebar"
        );
    }

    #[test]
    fn test_build_css_rules_count() {
        let rules = build_css_rules(&ColorPalette::VARS);
        // We expect a substantial number of CSS rules (roughly 40+)
        assert!(
            rules.len() >= 35,
            "Expected at least 35 CSS rules, got {}",
            rules.len()
        );
    }

    #[test]
    fn test_css_rule_selectors_use_constants() {
        let rules = build_css_rules(&ColorPalette::VARS);
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
    fn test_render_collapse_css_classes() {
        let css = render_styles();
        assert!(
            css.contains(".collapse-toggle"),
            "CSS should contain .collapse-toggle style"
        );
        assert!(
            css.contains(".collapsed"),
            "CSS should contain .collapsed style"
        );
        assert!(
            css.contains(".virtual-arc"),
            "CSS should contain .virtual-arc style"
        );
        assert!(
            css.contains(".reexport-arc"),
            "CSS should contain .reexport-arc style"
        );
        assert!(
            css.contains(".arc-count"),
            "CSS should contain .arc-count style"
        );
        assert!(
            css.contains(".child-count"),
            "CSS should contain .child-count style"
        );
    }

    #[test]
    fn test_search_active_dimming_rules_exist() {
        let css = render_styles();
        let sa = CSS.search.search_active;
        let sm = CSS.search.search_match;

        // Rect dimming rule with search-match exclusion
        let rect_rule = format!("svg.{sa} rect:not(.{sm})");
        assert!(
            css.contains(&rect_rule),
            "CSS should contain svg.search-active rect dimming rule"
        );

        // Path dimming rule with search-match exclusion
        let path_rule = format!("svg.{sa} path:not(.{sm})");
        assert!(
            css.contains(&path_rule),
            "CSS should contain svg.search-active path dimming rule"
        );

        // Polygon dimming rule with search-match exclusion
        let polygon_rule = format!("svg.{sa} polygon:not(.{sm})");
        assert!(
            css.contains(&polygon_rule),
            "CSS should contain svg.search-active polygon dimming rule"
        );

        // Line dimming rule
        let line_rule = format!("svg.{sa} line");
        assert!(
            css.contains(&line_rule),
            "CSS should contain svg.search-active line dimming rule"
        );
    }

    #[test]
    fn test_sidebar_content_has_min_height_zero() {
        let css = render_styles();
        // .sidebar-content needs min-height: 0 for robust flex shrinking
        // in foreignObject context (default min-height: auto prevents shrinking)
        let content_start = css
            .find(".sidebar-content")
            .expect(".sidebar-content rule should exist");
        let content_section = &css[content_start..content_start + 200];
        assert!(
            content_section.contains("min-height: 0"),
            ".sidebar-content should have min-height: 0, got: {content_section}"
        );
    }

    #[test]
    fn test_css_contains_cycle_block_rules() {
        let css = render_styles();
        for selector in [
            ".cycle-block",
            ".block-head",
            ".block-head::-webkit-details-marker",
            ".block-chevron",
            ".cycle-block[open] .block-chevron",
            ".block-ordinal",
            ".block-path",
            ".block-module-count",
            ".cycle-block-body",
            ".sidebar-edge-row.edge-repeat",
            ".sidebar-edge-row.edge-closing",
            ".sidebar-edge-closing-marker",
        ] {
            assert!(css.contains(selector), "CSS should contain {selector}");
        }
        // Closing row reuses the existing cycle color, no new red introduced.
        assert!(
            css.contains(&format!(
                ".sidebar-edge-row.edge-closing .sidebar-arrow {{ color: {};",
                ColorPalette::VARS.direction.cycle
            )),
            "edge-closing arrow should reuse the existing cycle color"
        );
        // Chevron rotation transition is gated behind reduced-motion opt-in.
        assert!(
            css.contains("@media (prefers-reduced-motion: no-preference)"),
            "chevron transition should be gated behind prefers-reduced-motion"
        );
    }

    #[test]
    fn test_arc_hitarea_css_class_exists() {
        let css = render_styles();
        // Hit-area CSS class must exist with correct properties
        assert!(
            css.contains(".arc-hitarea"),
            "CSS should contain .arc-hitarea class"
        );
        assert!(
            css.contains("pointer-events: stroke"),
            "arc-hitarea should have pointer-events: stroke"
        );
        // Visible arcs should have pointer-events: none
        assert!(
            css.contains(".dep-arc, .cycle-arc { pointer-events: none; }")
                || css.contains(".dep-arc, .cycle-arc {") && css.contains("pointer-events: none"),
            "dep-arc and cycle-arc should have pointer-events: none"
        );
    }

    /// A `.{class} { ... }` rule's own declarations, so a test can check the
    /// rule's own contents once instead of grepping the whole sheet, where a
    /// coincidental match elsewhere would pass it just as well.
    fn rule_body<'a>(css: &'a str, class: &str) -> &'a str {
        let start = css
            .find(&format!(".{class} {{"))
            .unwrap_or_else(|| panic!("the .{class} rule exists"));
        let end = css[start..]
            .find('}')
            .map_or_else(|| panic!("the .{class} rule is closed"), |i| start + i);
        &css[start..end]
    }

    /// A leaf paints more opaque than a container, so the file circles a
    /// hotspot leaf actually is stand out over the containers holding them.
    /// The leaf rule binds through both classes: its specificity beats the
    /// plain circle rule regardless of source order.
    #[test]
    fn a_leaf_paints_more_opaque_than_a_container() {
        let css = render_styles();

        let circle_rule = rule_body(&css, CSS.hotspots.circle);
        assert!(
            circle_rule.contains("fill-opacity: 0.32;"),
            "container circles need a lower fill-opacity, got: {circle_rule}"
        );
        assert!(
            css.contains(&format!(
                ".{}.{} {{ fill-opacity: 0.92; }}",
                CSS.hotspots.circle, CSS.hotspots.leaf
            )),
            "leaf circles need a higher fill-opacity, bound through both classes so it wins by \
             specificity rather than by coming second, got: {css}"
        );
    }

    /// Every circle keeps a faint hairline of its own, in the theme's
    /// outline colour, so a container circle is visible before it is
    /// hovered or ranked - much fainter than the outline, hover and
    /// selected rings, which all set full stroke-opacity.
    #[test]
    fn every_hotspot_circle_has_a_faint_hairline() {
        let css = render_styles();

        assert!(
            css.contains(&format!(
                ".{} {{ stroke: {}; stroke-opacity: .28; vector-effect: non-scaling-stroke; \
                 fill-opacity: 0.32; }}",
                CSS.hotspots.circle,
                ColorPalette::VARS.hotspots.outline,
            )),
            "got: {css}"
        );
    }

    /// A label's text carries a halo in the page background colour, so it
    /// stays legible over any fill it sits on, with rounded corners so the
    /// browser's default mitre join does not spike them.
    #[test]
    fn a_labels_text_carries_a_rounded_background_halo() {
        let css = render_styles();

        let label_rule_start = css
            .find(&format!(".{} {{", CSS.hotspots.label))
            .expect("the label rule exists");
        let label_rule_end = css[label_rule_start..]
            .find('}')
            .map(|i| label_rule_start + i)
            .expect("the label rule is closed");
        let label_rule = &css[label_rule_start..label_rule_end];

        assert!(
            label_rule.contains("paint-order: stroke;"),
            "got: {label_rule}"
        );
        assert!(
            label_rule.contains("stroke: var(--arc-page-bg);"),
            "got: {label_rule}"
        );
        assert!(
            label_rule.contains("stroke-width: .22em;"),
            "got: {label_rule}"
        );
        assert!(
            label_rule.contains("stroke-linejoin: round;"),
            "got: {label_rule}"
        );
    }

    /// The jump glyph sits on leaf circles whose fill can be the hottest
    /// colour in the ramp, so it carries the same halo as a label, with the
    /// same values, so the two stay in step. Checked by slicing both rules
    /// and asserting the icon's own declarations inside the label rule too,
    /// so a value drifting in one but not the other fails this test instead
    /// of two hardcoded literals passing regardless of each other.
    #[test]
    fn the_jump_icon_carries_the_same_halo_as_a_label() {
        let css = render_styles();

        let label_rule = rule_body(&css, CSS.hotspots.label);
        let icon_rule = rule_body(&css, CSS.hotspots.jump_icon);

        for declaration in [
            "paint-order: stroke;".to_string(),
            format!("stroke: {};", ColorPalette::VARS.page.bg),
            "stroke-width: .22em;".to_string(),
            "stroke-linejoin: round;".to_string(),
            "font-size: 11px;".to_string(),
        ] {
            assert!(
                icon_rule.contains(&declaration),
                "expected the icon rule to declare {declaration}, got: {icon_rule}"
            );
            assert!(
                label_rule.contains(&declaration),
                "expected the label rule to also declare {declaration}, got: {label_rule}"
            );
        }
    }

    /// A ranked hotspot's outline is thick enough to read on its own screen
    /// pixels now that `non-scaling-stroke` no longer lets it grow with zoom,
    /// and thinner than the hover ring so a hover still visibly changes it.
    /// Full stroke-opacity, so the base circle rule's faint hairline does
    /// not leave it any fainter than that.
    #[test]
    fn a_hotspots_outline_is_three_screen_pixels_wide() {
        let css = render_styles();

        assert!(
            css.contains(&format!(
                ".{} {{ stroke: {}; stroke-width: 3; stroke-opacity: 1; \
                 vector-effect: non-scaling-stroke; }}",
                CSS.hotspots.outline,
                ColorPalette::VARS.hotspots.outline,
            )),
            "got: {css}"
        );
    }

    /// Hovering a ranked hotspot must visibly change it beyond the colour:
    /// the hover ring is wider than the always-visible outline and the
    /// selected ring, the same relation the prototype's own variant draws.
    #[test]
    fn a_hovered_hotspots_ring_is_wider_than_its_outline_and_selection() {
        let css = render_styles();

        assert!(
            css.contains(&format!(
                ".{} {{ stroke: {}; stroke-width: 4; stroke-opacity: 1; \
                 vector-effect: non-scaling-stroke; }}",
                CSS.hotspots.hover,
                ColorPalette::VARS.hotspots.highlight,
            )),
            "hover must be wider than the outline (3) and the selected ring (3.5), got: {css}"
        );
    }
}
