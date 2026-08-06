use super::constants::{CSS, LAYOUT};
use super::positioning::PositionedItem;
use crate::layout::{CycleKind, EdgeDirection, ItemKind, LayoutEdge, LayoutIR, NodeId};
use crate::model::DependencyKind;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

pub(super) fn render_header(width: f32, height: f32) -> String {
    // cluster-mode-on defaults on (cycles checkbox checked), matching the other
    // root state classes (has-highlight/has-pinned) that JS toggles on the SVG.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" class="{}" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
"#,
        CSS.relation.cluster_mode_on
    )
}

#[allow(clippy::cast_possible_truncation)] // SVG pixel coordinates fit in i32
pub(super) fn render_sidebar(width: f32) -> String {
    let x = if width > 280.0 {
        (width - 280.0) as i32
    } else {
        0
    };
    let cs = &CSS.sidebar;
    // overflow:visible lets box-shadow and border-radius render outside the
    // foreignObject boundary (SVG foreignObject defaults to overflow:hidden).
    // Initial height 500 — JS updatePosition() resizes dynamically to content/viewport
    format!(
        concat!(
            "<foreignObject id=\"relation-sidebar\" x=\"{}\" y=\"0\" width=\"280\" height=\"500\" style=\"display:none; overflow:visible\">\n",
            "  <div class=\"{}\" xmlns=\"http://www.w3.org/1999/xhtml\"></div>\n",
            "</foreignObject>\n",
        ),
        x, cs.root,
    )
}

#[allow(clippy::cast_possible_truncation)] // SVG pixel coordinates fit in i32
#[allow(clippy::too_many_lines)] // single cohesive markup template
pub(super) fn render_toolbar(
    width: f32,
    has_externals: bool,
    has_transitive_externals: bool,
    initial_collapsed: bool,
) -> String {
    let ct = &CSS.toolbar;
    let height = LAYOUT.toolbar.height as i32;

    let transitive_checkbox = if has_transitive_externals {
        format!(
            concat!(
                "          <label class=\"{}\">\n",
                "            <span class=\"{} {}\" id=\"transitive-dep-checkbox\"></span>\n",
                "            Transitive Dependencies\n",
                "          </label>\n",
            ),
            ct.toggle, ct.checkbox, ct.checked,
        )
    } else {
        String::new()
    };

    let external_checkbox = if has_externals {
        format!(
            concat!(
                "          <label class=\"{}\">\n",
                "            <span class=\"{} {}\" id=\"external-dep-checkbox\"></span>\n",
                "            External Dependencies\n",
                "          </label>\n",
                "{}",
            ),
            ct.toggle, ct.checkbox, ct.checked, transitive_checkbox,
        )
    } else {
        String::new()
    };

    format!(
        concat!(
            "  <foreignObject id=\"toolbar-fo\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"",
            " style=\"display:none; overflow:visible\">\n",
            "    <div class=\"{}\" xmlns=\"http://www.w3.org/1999/xhtml\">\n",
            "      <button id=\"collapse-toggle-btn\" class=\"{}\">{}</button>\n",
            "      <span class=\"{}\"></span>\n",
            "      <div class=\"{}\">\n",
            "        <button id=\"view-dropdown-btn\" class=\"{} {}\">View \u{25be}</button>\n",
            "        <div class=\"{}\" style=\"display:none\">\n",
            "          <label class=\"{}\">\n",
            "            <span class=\"{} {}\" id=\"crate-dep-checkbox\"></span>\n",
            "            Show Crate Dependencies\n",
            "          </label>\n",
            "          <label class=\"{}\">\n",
            "            <span class=\"{} {}\" id=\"module-dep-checkbox\"></span>\n",
            "            Show Module Dependencies\n",
            "          </label>\n",
            "          <label class=\"{}\">\n",
            "            <span class=\"{}\" id=\"reexport-dep-checkbox\"></span>\n",
            "            Show Re-Export Dependencies\n",
            "          </label>\n",
            "          <label class=\"{}\">\n",
            "            <span class=\"{} {}\" id=\"cycles-checkbox\"></span>\n",
            "            Show Circular Dependencies\n",
            "          </label>\n",
            "{}",
            "        </div>\n",
            "      </div>\n",
            "      <span class=\"{}\"></span>\n",
            "      <div class=\"{}\">\n",
            "        <div class=\"{}\">\n",
            "          <input id=\"search-input\" type=\"text\" placeholder=\"Search...\" />\n",
            "          <button id=\"search-clear\" class=\"{}\"",
            " style=\"display:none\">\u{2715}</button>\n",
            "        </div>\n",
            "        <div id=\"scope-selector\" class=\"{}\">\n",
            "          <button class=\"{} {}\" data-scope=\"all\">All</button>\n",
            "          <button class=\"{}\" data-scope=\"crate\">Crate</button>\n",
            "          <button class=\"{}\" data-scope=\"module\">Module</button>\n",
            "          <button class=\"{}\" data-scope=\"symbol\">Symbol</button>\n",
            "        </div>\n",
            "        <span id=\"search-result-count\" class=\"{}\"></span>\n",
            "      </div>\n",
            "    </div>\n",
            "  </foreignObject>\n",
        ),
        width,       // foreignObject width
        height,      // foreignObject height
        ct.root,     // .toolbar-root
        ct.html_btn, // collapse button class
        if initial_collapsed {
            "Expand All"
        } else {
            "Collapse All"
        }, // button text
        ct.separator_v, // separator
        ct.dropdown, // .toolbar-dropdown container
        ct.html_btn, // dropdown button base class
        ct.dropdown_btn, // dropdown button marker class
        ct.dropdown_panel, // .toolbar-dropdown-panel
        ct.toggle,   // label.toolbar-toggle (crate dep)
        ct.checkbox,
        ct.checked, // checkbox span (checked)
        ct.toggle,  // label.toolbar-toggle (module dep)
        ct.checkbox,
        ct.checked,  // checkbox span (checked)
        ct.toggle,   // label.toolbar-toggle (re-export dep)
        ct.checkbox, // checkbox span (unchecked → default hidden)
        ct.toggle,   // label.toolbar-toggle (cycles)
        ct.checkbox,
        ct.checked,              // checkbox span (checked → cluster mode on)
        external_checkbox,       // optional external dep checkbox
        ct.separator_v,          // separator
        ct.search_group,         // .toolbar-search-group
        ct.search_input_wrapper, // .toolbar-search-input-wrapper
        ct.search_clear,         // .toolbar-search-clear
        ct.scope,                // .toolbar-scope
        ct.scope_btn,
        ct.scope_active, // first scope btn (active)
        ct.scope_btn,    // crate scope btn
        ct.scope_btn,    // module scope btn
        ct.scope_btn,    // symbol scope btn
        ct.result_count, // .toolbar-result-count
    )
}

pub(super) fn render_tree_lines(
    positioned_index: &HashMap<NodeId, &PositionedItem>,
    ir: &LayoutIR,
) -> String {
    let mut lines = String::new();
    lines.push_str("  <g id=\"tree-lines\">\n");

    // Find children for each parent
    for item in &ir.items {
        if let ItemKind::Module { parent, .. } = &item.kind {
            let parent_pos = positioned_index.get(parent).copied();
            let child_pos = positioned_index.get(&item.id).copied();

            if let (Some(parent_pos), Some(child_pos)) = (parent_pos, child_pos) {
                let line_x = parent_pos.x + LAYOUT.tree_line_x_offset;
                let parent_bottom = parent_pos.y + parent_pos.height;
                let child_mid_y = child_pos.y + child_pos.height / 2.0;

                let data_attrs = format!(r#" data-parent="{}" data-child="{}""#, parent, item.id);
                let tl = CSS.nodes.tree_line;

                let _ = writeln!(
                    lines,
                    "    <line class=\"{tl}\" x1=\"{line_x}\" y1=\"{parent_bottom}\" x2=\"{line_x}\" y2=\"{child_mid_y}\"{data_attrs}/>"
                );

                let child_left = child_pos.x;
                let _ = writeln!(
                    lines,
                    "    <line class=\"{tl}\" x1=\"{line_x}\" y1=\"{child_mid_y}\" x2=\"{child_left}\" y2=\"{child_mid_y}\"{data_attrs}/>"
                );
            }
        }
    }

    lines.push_str("  </g>\n");
    lines
}

pub(super) fn render_nodes(
    positioned: &[PositionedItem],
    parents: &HashSet<NodeId>,
    visible_nodes: Option<&HashSet<NodeId>>,
    collapsed_parents: &HashSet<NodeId>,
    visible_index: &HashMap<NodeId, &PositionedItem>,
) -> String {
    let mut nodes = String::new();
    nodes.push_str("  <g id=\"nodes\">\n");

    for item in positioned {
        // Hidden nodes get collapsed class and use off-screen position
        let is_hidden = visible_nodes.is_some_and(|v| !v.contains(&item.id));
        let is_collapsed_parent = collapsed_parents.contains(&item.id);

        // Use visible position if available, otherwise original
        let (render_x, render_y) = if let Some(vis_pos) = visible_index.get(&item.id) {
            (vis_pos.x, vis_pos.y)
        } else {
            (item.x, item.y)
        };
        let class = match &item.kind {
            ItemKind::Crate => CSS.nodes.crate_node,
            ItemKind::Module { .. } => CSS.nodes.module,
            ItemKind::ExternalSection => CSS.nodes.external_section,
            ItemKind::ExternalCrate {
                is_direct_dependency: true,
                ..
            } => CSS.nodes.external_crate,
            ItemKind::ExternalCrate {
                is_direct_dependency: false,
                ..
            } => CSS.nodes.external_transitive,
        };
        let rx = match &item.kind {
            ItemKind::Crate | ItemKind::ExternalSection => LAYOUT.crate_border_radius,
            ItemKind::Module { .. } | ItemKind::ExternalCrate { .. } => LAYOUT.module_border_radius,
        };

        // data-parent attribute for modules and external crates
        let parent_attr = match &item.kind {
            ItemKind::Module { parent, .. } | ItemKind::ExternalCrate { parent, .. } => {
                format!(r#" data-parent="{parent}""#)
            }
            ItemKind::Crate | ItemKind::ExternalSection => String::new(),
        };

        // data-has-children attribute for parents
        let has_children_attr = if parents.contains(&item.id) {
            r#" data-has-children="true""#
        } else {
            ""
        };

        // CSS class: add "collapsed" for hidden nodes
        let collapsed_cls = CSS.nodes.collapsed;
        let full_class = if is_hidden {
            format!("{class} {collapsed_cls}")
        } else {
            class.to_string()
        };

        let label = escape_xml(&item.label);
        let text_x = render_x + LAYOUT.text_padding_x;
        let text_y = render_y + item.height / 2.0 + LAYOUT.text_y_offset;

        let _ = writeln!(
            nodes,
            "    <rect class=\"{full_class}\" id=\"node-{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{rx}\"{parent_attr}{has_children_attr}/>",
            item.id, render_x, render_y, item.width, item.height
        );

        // Label with optional child-count tspan for parents
        let lbl = CSS.nodes.label;
        let cc = CSS.nodes.child_count;
        let label_class = if is_hidden {
            format!("{lbl} {collapsed_cls}")
        } else {
            lbl.to_string()
        };
        if parents.contains(&item.id) {
            // Show child count for collapsed parents
            let count_text = if is_collapsed_parent {
                format!(" (+{})", child_count(positioned, item.id))
            } else {
                String::new()
            };
            let _ = writeln!(
                nodes,
                "    <text class=\"{label_class}\" x=\"{text_x}\" y=\"{text_y}\">{label}<tspan id=\"count-{}\" class=\"{cc}\">{count_text}</tspan></text>",
                item.id
            );
        } else {
            let _ = writeln!(
                nodes,
                "    <text class=\"{label_class}\" x=\"{text_x}\" y=\"{text_y}\">{label}</text>"
            );
        }

        // Toggle icon (+/-) for parents
        if parents.contains(&item.id) {
            nodes.push_str(&render_collapse_toggle(
                item,
                render_x,
                render_y,
                is_collapsed_parent,
                is_hidden,
            ));
        }
    }

    nodes.push_str("  </g>\n");
    nodes
}

fn child_count(positioned: &[PositionedItem], parent_id: NodeId) -> usize {
    positioned
        .iter()
        .filter(|p| match &p.kind {
            ItemKind::Module { parent, .. } | ItemKind::ExternalCrate { parent, .. } => {
                *parent == parent_id
            }
            ItemKind::Crate | ItemKind::ExternalSection => false,
        })
        .count()
}

fn render_collapse_toggle(
    item: &PositionedItem,
    render_x: f32,
    render_y: f32,
    is_collapsed_parent: bool,
    is_hidden: bool,
) -> String {
    let toggle_x = render_x + item.width - LAYOUT.toggle_offset;
    let toggle_y = render_y + item.height / 2.0 + LAYOUT.toggle_y_offset;
    let ct = CSS.nodes.collapse_toggle;
    let toggle_icon = if is_collapsed_parent { "+" } else { "−" };
    let toggle_cls = if is_hidden {
        format!("{ct} {}", CSS.nodes.collapsed)
    } else {
        ct.to_string()
    };
    format!(
        "    <text class=\"{toggle_cls}\" data-target=\"{}\" x=\"{toggle_x}\" y=\"{toggle_y}\">{toggle_icon}</text>\n",
        item.id
    )
}

pub(super) fn render_edges(
    positioned_index: &HashMap<NodeId, &PositionedItem>,
    ir: &LayoutIR,
    row_height: f32,
    visible_nodes: Option<&HashSet<NodeId>>,
) -> String {
    let mut base_arcs = String::new();
    let mut hitareas = String::new();

    // Find the rightmost edge of all nodes for base arc position
    let base_x = positioned_index
        .values()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, f32::max);

    // Sort edges by type for z-order: Test/Build (back) → Downward Production →
    // Upward Production → Cycle (front). In SVG, later elements render on top.
    let mut edge_order: Vec<usize> = (0..ir.edges.len()).collect();
    edge_order.sort_by_key(|&i| {
        let edge = &ir.edges[i];
        match (edge.cycle, edge.direction, &edge.context.kind) {
            (_, _, DependencyKind::Test(_) | DependencyKind::Build) => 0,
            (None, EdgeDirection::Downward, DependencyKind::Production) => 1,
            (None, EdgeDirection::Upward, DependencyKind::Production) => 2,
            (Some(_), _, _) => 3,
        }
    });

    for &idx in &edge_order {
        let edge = &ir.edges[idx];
        // Skip edges to/from hidden nodes
        if let Some(visible) = visible_nodes
            && (!visible.contains(&edge.from) || !visible.contains(&edge.to))
        {
            continue;
        }
        let from_pos = positioned_index.get(&edge.from).copied();
        let to_pos = positioned_index.get(&edge.to).copied();

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
            let mid_y = f32::midpoint(from_y, to_y);

            // S-shaped Bezier with two Q commands
            let path = format!(
                "M {from_x},{from_y} Q {ctrl_x},{from_y} {ctrl_x},{mid_y} Q {ctrl_x},{to_y} {to_x},{to_y}"
            );

            let ArcAttrs {
                arc: arc_class,
                arrow: arrow_class,
                hitarea,
                direction,
            } = arc_attrs(edge, &from.kind, &to.kind);

            let edge_id = format!("{}-{}", edge.from, edge.to);
            let cycle_ids_attr = if edge.cycle_ids.is_empty() {
                String::new()
            } else {
                let ids: Vec<String> = edge
                    .cycle_ids
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect();
                format!(r#" data-cycle-ids="{}""#, ids.join(","))
            };

            // Hit-area path (invisible, 12px wide, receives pointer events) → hitareas layer
            // Note: source-locations are read from STATIC_DATA in JavaScript, not DOM attributes
            let _ = writeln!(
                hitareas,
                "    <path class=\"{hitarea}\" data-arc-id=\"{edge_id}\" data-from=\"{}\" data-to=\"{}\" data-direction=\"{direction}\"{cycle_ids_attr} d=\"{path}\"/>",
                edge.from, edge.to
            );
            // Visible path (styled, no pointer events) → base-arcs layer
            let _ = writeln!(
                base_arcs,
                "    <path class=\"{arc_class}\" id=\"edge-{edge_id}\" data-arc-id=\"{edge_id}\" data-direction=\"{direction}\"{cycle_ids_attr} d=\"{path}\"/>"
            );

            // Arrow head pointing to target → base-arcs layer
            let arrow = render_arrow(to_x, to_y, &arrow_class, &edge_id);
            base_arcs.push_str(&arrow);

            // For DirectCycle, add reverse arrow.
            if edge.cycle == Some(CycleKind::Direct) {
                let reverse_arrow = render_arrow(from_x, from_y, &arrow_class, &edge_id);
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
{base_arcs}  </g>
  <g id="base-labels-layer"></g>
  <g id="highlight-shadows"></g>
  <g id="highlight-arcs-layer"></g>
  <g id="highlight-labels-layer"></g>
  <g id="hitareas-layer">
{hitareas}  </g>
  <g id="highlight-hitareas-layer"></g>
"#
    )
}

/// The three CSS class lists an arc's elements carry, plus its `data-direction` value.
struct ArcAttrs {
    arc: String,
    arrow: String,
    hitarea: String,
    direction: &'static str,
}

fn arc_attrs(edge: &LayoutEdge, from_kind: &ItemKind, to_kind: &ItemKind) -> ArcAttrs {
    let cd = &CSS.direction;
    // Every edge carries its directional dep classes so that, with
    // cluster mode off, cycle edges fall back to the normal dependency
    // color. cycle-arc/cycle-arrow are additive markers the container
    // state (.cluster-mode-on) styles red.
    let (dir_arc_class, dir_arrow_class, direction) = match edge.direction {
        EdgeDirection::Downward => (cd.downward, cd.dep_arrow, "downward"),
        EdgeDirection::Upward => (cd.upward, cd.upward_arrow, "upward"),
    };
    let (arc_cycle_marker, arrow_cycle_marker) = if edge.cycle.is_some() {
        (format!(" {}", cd.cycle_arc), format!(" {}", cd.cycle_arrow))
    } else {
        (String::new(), String::new())
    };

    // Add crate-dep-arc, module-dep-arc, or reexport-arc class based on edge type.
    // Re-export arcs are their own category (default hidden) so their toggle
    // stays independent of the module-dep toggle.
    let is_crate_dep = matches!(
        (from_kind, to_kind),
        (
            ItemKind::Crate | ItemKind::ExternalCrate { .. },
            ItemKind::Crate | ItemKind::ExternalCrate { .. }
        )
    );
    let arc_type_class = if edge.reexport {
        cd.reexport_arc
    } else if is_crate_dep {
        cd.crate_dep_arc
    } else {
        cd.module_dep_arc
    };
    // Re-export arcs start hidden; the toolbar checkbox (unchecked) reveals them.
    let hidden = if edge.reexport {
        format!(" {}", CSS.labels.hidden_by_filter)
    } else {
        String::new()
    };

    ArcAttrs {
        arc: format!(
            "{} {dir_arc_class}{arc_cycle_marker} {arc_type_class}{hidden}",
            cd.dep_arc
        ),
        arrow: format!("{dir_arrow_class}{arrow_cycle_marker}{hidden}"),
        hitarea: format!("{}{hidden}", cd.arc_hitarea),
        direction,
    }
}

fn render_arrow(x: f32, y: f32, class: &str, edge_id: &str) -> String {
    // Arrow pointing left (toward the node at x)
    // Tip at x, base at x+8
    let p1 = format!(
        "{},{}",
        x + LAYOUT.arrow_length,
        y - LAYOUT.arrow_length / 2.0
    ); // top-right
    let p2 = format!("{x},{y}"); // tip (left, pointing at node)
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
    use super::super::constants::RenderConfig;
    use super::super::positioning::{calculate_box_width, calculate_positions};
    use super::*;
    use crate::layout::LayoutEdge;
    use crate::model::EdgeContext;
    use std::collections::HashMap;

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
    fn test_toolbar_has_cycles_checkbox_checked_by_default() {
        let toolbar = render_toolbar(800.0, false, false, false);
        let idx = toolbar
            .find("id=\"cycles-checkbox\"")
            .expect("Toolbar should render the cycles checkbox");
        let span_start = toolbar[..idx]
            .rfind("<span")
            .expect("cycles checkbox should be a span");
        assert!(
            toolbar[span_start..idx].contains(CSS.toolbar.checked),
            "Cycles checkbox should have the checked class by default (cluster mode on)"
        );
    }

    #[test]
    fn test_xml_escaping() {
        let escaped = escape_xml("foo<bar>&baz");
        assert_eq!(escaped, "foo&lt;bar&gt;&amp;baz");
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
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_tree_lines(&positioned_index, &ir);
        assert!(output.contains("tree-line"));
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
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_tree_lines(&positioned_index, &ir);
        assert!(
            output.contains(r#"class="tree-line""#) && output.contains(r#"data-parent="0""#),
            "Tree lines should have data-parent attribute"
        );
        assert!(
            output.contains(r#"data-child="1""#),
            "Tree lines should have data-child attribute"
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
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let parents: HashSet<NodeId> = [c].into();
        let positioned_index: HashMap<NodeId, &PositionedItem> =
            positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_nodes(
            &positioned,
            &parents,
            None,
            &HashSet::new(),
            &positioned_index,
        );
        assert!(output.contains(r#"id="node-0""#), "Crate should have id");
        assert!(output.contains(r#"id="node-1""#), "Module should have id");
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
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let parents: HashSet<NodeId> = [c].into();
        let positioned_index: HashMap<NodeId, &PositionedItem> =
            positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_nodes(
            &positioned,
            &parents,
            None,
            &HashSet::new(),
            &positioned_index,
        );
        assert!(
            output.contains(r#"data-parent="0""#),
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
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let parents: HashSet<NodeId> = [c].into();
        let positioned_index: HashMap<NodeId, &PositionedItem> =
            positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_nodes(
            &positioned,
            &parents,
            None,
            &HashSet::new(),
            &positioned_index,
        );
        assert!(
            output.contains(r#"data-has-children="true""#),
            "Crate with children should have data-has-children attribute"
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
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let parents: HashSet<NodeId> = [c].into();
        let positioned_index: HashMap<NodeId, &PositionedItem> =
            positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_nodes(
            &positioned,
            &parents,
            None,
            &HashSet::new(),
            &positioned_index,
        );
        assert!(
            output.contains(r#"class="collapse-toggle""#),
            "Parent nodes should have collapse toggle"
        );
        assert!(
            output.contains(r#"data-target="0""#),
            "Collapse toggle should target parent node"
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
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let parents: HashSet<NodeId> = [c].into();
        let positioned_index: HashMap<NodeId, &PositionedItem> =
            positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_nodes(
            &positioned,
            &parents,
            None,
            &HashSet::new(),
            &positioned_index,
        );
        assert!(
            output.contains(r#"id="count-0""#),
            "Parent should have child-count tspan with id"
        );
        assert!(
            output.contains(r#"class="child-count""#),
            "Tspan should have child-count class"
        );
    }

    #[test]
    fn test_render_collapsed_parent_counts_children_and_offers_expand() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "parent".into());
        for child in ["first", "second"] {
            ir.add_item(
                ItemKind::Module {
                    nesting: 1,
                    parent: c,
                },
                child.into(),
            );
        }
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let parents: HashSet<NodeId> = [c].into();
        let collapsed: HashSet<NodeId> = [c].into();
        let positioned_index: HashMap<NodeId, &PositionedItem> =
            positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_nodes(&positioned, &parents, None, &collapsed, &positioned_index);
        assert!(
            output.contains(r#"class="child-count"> (+2)</tspan>"#),
            "Collapsed parent should report how many children it hides"
        );
        assert!(
            output.contains(">+</text>"),
            "Collapsed parent should show the expand icon"
        );
    }

    #[test]
    fn test_render_toolbar_contains_elements() {
        let output = render_toolbar(800.0, false, false, false);
        assert!(
            output.contains(r#"id="toolbar-fo""#),
            "Should have foreignObject with toolbar-fo id"
        );
        assert!(
            output.contains(&format!(r#"class="{}""#, CSS.toolbar.root)),
            "Should have toolbar-root div"
        );
        assert!(
            output.contains(r#"id="collapse-toggle-btn""#),
            "Should have collapse toggle button"
        );
        assert!(
            output.contains("Collapse All"),
            "Should have 'Collapse All' text"
        );
        assert!(
            output.contains(r#"id="view-dropdown-btn""#),
            "Should have View dropdown button"
        );
        assert!(
            output.contains(&format!(r#"class="{}""#, CSS.toolbar.dropdown)),
            "Should have dropdown container"
        );
        assert!(
            output.contains(&format!(r#"class="{}""#, CSS.toolbar.dropdown_panel)),
            "Should have dropdown panel"
        );
        assert!(
            output.contains(r#"id="crate-dep-checkbox""#),
            "Should have crate-dep checkbox"
        );
        assert!(
            output.contains("Show Crate Dependencies"),
            "Should have crate dependency label"
        );
        assert!(
            output.contains(r#"id="module-dep-checkbox""#),
            "Should have module-dep checkbox"
        );
        assert!(
            output.contains("Show Module Dependencies"),
            "Should have module dependency label"
        );
        assert!(
            output.contains(r#"id="search-input""#),
            "Should have search input"
        );
        assert!(
            output.contains(r#"id="scope-selector""#),
            "Should have scope selector"
        );
        assert!(
            output.contains(r#"id="search-result-count""#),
            "Should have search result count"
        );
        assert!(
            output.contains("xmlns=\"http://www.w3.org/1999/xhtml\""),
            "Should have XHTML namespace"
        );
    }

    #[test]
    fn test_render_toolbar_external_checkbox_when_externals_present() {
        let output = render_toolbar(800.0, true, false, false);
        assert!(
            output.contains(r#"id="external-dep-checkbox""#),
            "Should have external-dep checkbox when externals present"
        );
        assert!(
            output.contains("External Dependencies"),
            "Should have external dependency label"
        );
    }

    #[test]
    fn test_render_toolbar_no_external_checkbox_without_externals() {
        let output = render_toolbar(800.0, false, false, false);
        assert!(
            !output.contains(r#"id="external-dep-checkbox""#),
            "Should NOT have external-dep checkbox without externals"
        );
    }

    #[test]
    fn test_render_toolbar_transitive_checkbox_when_transitive_present() {
        let output = render_toolbar(800.0, true, true, false);
        assert!(
            output.contains(r#"id="transitive-dep-checkbox""#),
            "Should have transitive-dep checkbox when transitive externals present"
        );
        assert!(
            output.contains("Transitive Dependencies"),
            "Should have transitive dependency label"
        );
    }

    #[test]
    fn test_render_toolbar_no_transitive_checkbox_without_transitive() {
        let output = render_toolbar(800.0, true, false, false);
        assert!(
            !output.contains(r#"id="transitive-dep-checkbox""#),
            "Should NOT have transitive-dep checkbox without transitive externals"
        );
    }

    #[test]
    fn test_render_toolbar_no_transitive_checkbox_without_externals() {
        let output = render_toolbar(800.0, false, true, false);
        assert!(
            !output.contains(r#"id="transitive-dep-checkbox""#),
            "Transitive checkbox should be nested inside external checkbox block"
        );
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();

        let output = render_edges(&positioned_index, &ir, config.row_height, None);
        assert!(output.contains(r#"id="edge-1-2""#), "Edge should have id");
        assert!(
            output.contains(r#"data-from="1""#),
            "Edge should have data-from"
        );
        assert!(
            output.contains(r#"data-to="2""#),
            "Edge should have data-to"
        );
        assert!(
            output.contains(r#"data-direction="downward""#),
            "Edge should have data-direction"
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();

        let output = render_edges(&positioned_index, &ir, config.row_height, None);

        assert!(
            output.contains(r#"class="arc-hitarea""#),
            "Should have hit-area path"
        );
        assert!(
            output.contains(r#"class="dep-arc downward module-dep-arc""#),
            "Should have visible dep-arc path with direction and module-dep-arc class"
        );
        assert!(
            output.contains(r#"data-arc-id="1-2""#),
            "Both paths should have data-arc-id"
        );

        let hitarea_line = output
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
    fn test_crate_dep_edges_have_class() {
        let mut ir = LayoutIR::new();
        let c1 = ir.add_item(ItemKind::Crate, "crate_a".into());
        let c2 = ir.add_item(ItemKind::Crate, "crate_b".into());
        ir.edges
            .push(LayoutEdge::new(c1, c2, EdgeContext::production()));
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();

        let output = render_edges(&positioned_index, &ir, config.row_height, None);
        assert!(
            output.contains("crate-dep-arc"),
            "Crate-to-crate edges should have crate-dep-arc class"
        );
    }

    #[test]
    fn test_reexport_edges_have_teal_class_default_hidden() {
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
        let mut edge = LayoutEdge::new(a, b, EdgeContext::production());
        edge.reexport = true;
        ir.edges.push(edge);
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();

        let output = render_edges(&positioned_index, &ir, config.row_height, None);

        // Visible arc carries reexport-arc + hidden-by-filter, not module-dep-arc
        let arc_line = output
            .lines()
            .find(|l| l.contains(r#"id="edge-1-2""#))
            .expect("Should find arc path for edge 1-2");
        assert!(
            arc_line.contains("reexport-arc"),
            "Re-export arc should have reexport-arc class, got: {arc_line}"
        );
        assert!(
            !arc_line.contains("module-dep-arc"),
            "Re-export arc should NOT have module-dep-arc class, got: {arc_line}"
        );
        assert!(
            arc_line.contains("hidden-by-filter"),
            "Re-export arc should start hidden, got: {arc_line}"
        );
        // Hitarea also default hidden
        let hitarea_line = output
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains(r#"data-arc-id="1-2""#))
            .expect("Should find hitarea for edge 1-2");
        assert!(
            hitarea_line.contains("hidden-by-filter"),
            "Re-export hitarea should start hidden, got: {hitarea_line}"
        );
    }

    #[test]
    fn test_render_toolbar_has_reexport_checkbox_unchecked() {
        let output = render_toolbar(800.0, false, false, false);
        assert!(
            output.contains(r#"id="reexport-dep-checkbox""#),
            "Should have reexport-dep checkbox"
        );
        assert!(
            output.contains("Show Re-Export Dependencies"),
            "Should have re-export dependency label"
        );
        // Checkbox span must not carry the checked class (default hidden)
        let cb_line = output
            .lines()
            .find(|l| l.contains(r#"id="reexport-dep-checkbox""#))
            .expect("Should find reexport checkbox span");
        assert!(
            !cb_line.contains(CSS.toolbar.checked),
            "Re-export checkbox should start unchecked, got: {cb_line}"
        );
    }

    #[test]
    fn test_data_cycle_ids_attribute() {
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
        let m = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "m".into(),
        );
        // Cycle edge with cycle_ids=[0]
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                CycleKind::Direct,
                vec![0],
                0,
            ));
        // Non-cycle edge
        ir.edges
            .push(LayoutEdge::new(a, m, EdgeContext::production()));

        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();

        let output = render_edges(&positioned_index, &ir, config.row_height, None);

        // Cycle arc path should have data-cycle-ids="0"
        let cycle_path = output
            .lines()
            .find(|l| l.contains("cycle-arc") && l.contains("id=\"edge-1-2\""))
            .expect("Should find cycle-arc path for edge 1-2");
        assert!(
            cycle_path.contains(r#"data-cycle-ids="0""#),
            "Cycle arc path should have data-cycle-ids attribute, got: {cycle_path}"
        );

        // Hitarea for cycle arc should also have data-cycle-ids
        let cycle_hitarea = output
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains(r#"data-arc-id="1-2""#))
            .expect("Should find hitarea for edge 1-2");
        assert!(
            cycle_hitarea.contains(r#"data-cycle-ids="0""#),
            "Cycle arc hitarea should have data-cycle-ids attribute, got: {cycle_hitarea}"
        );

        // Non-cycle arc should NOT have data-cycle-ids
        let normal_path = output
            .lines()
            .find(|l| l.contains("id=\"edge-1-3\""))
            .expect("Should find normal arc path for edge 1-3");
        assert!(
            !normal_path.contains("data-cycle-ids"),
            "Non-cycle arc should NOT have data-cycle-ids, got: {normal_path}"
        );

        // Non-cycle hitarea should NOT have data-cycle-ids
        let normal_hitarea = output
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains(r#"data-arc-id="1-3""#))
            .expect("Should find hitarea for edge 1-3");
        assert!(
            !normal_hitarea.contains("data-cycle-ids"),
            "Non-cycle hitarea should NOT have data-cycle-ids, got: {normal_hitarea}"
        );
    }

    #[test]
    fn test_cycle_edge_carries_direction_and_dep_class() {
        // Cycle edges keep dep-arc + direction classes so that, with cluster
        // mode off, they fall back to the normal directional dependency color.
        // cycle-arc is an additive marker gated by the container state.
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
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                CycleKind::Direct,
                vec![0],
                0,
            ));

        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_edges(&positioned_index, &ir, config.row_height, None);

        let cycle_path = output
            .lines()
            .find(|l| l.contains("id=\"edge-1-2\""))
            .expect("Should find visible path for edge 1-2");
        assert!(
            cycle_path.contains(CSS.direction.dep_arc)
                && cycle_path.contains(CSS.direction.downward)
                && cycle_path.contains(CSS.direction.cycle_arc),
            "Cycle edge should carry dep-arc + downward + cycle-arc, got: {cycle_path}"
        );
    }

    #[test]
    fn test_multi_cycle_ids_attribute() {
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
        // Edge belonging to two cycles
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                CycleKind::Direct,
                vec![0, 2],
                0,
            ));

        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();
        let output = render_edges(&positioned_index, &ir, config.row_height, None);

        // Visible path should have comma-separated cycle IDs
        let cycle_path = output
            .lines()
            .find(|l| l.contains("cycle-arc") && l.contains("id=\"edge-1-2\""))
            .expect("Should find cycle-arc path for edge 1-2");
        assert!(
            cycle_path.contains(r#"data-cycle-ids="0,2""#),
            "Multi-cycle arc should have comma-separated data-cycle-ids, got: {cycle_path}"
        );

        // Hitarea should also have comma-separated cycle IDs
        let hitarea = output
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains(r#"data-arc-id="1-2""#))
            .expect("Should find hitarea for edge 1-2");
        assert!(
            hitarea.contains(r#"data-cycle-ids="0,2""#),
            "Multi-cycle hitarea should have comma-separated data-cycle-ids, got: {hitarea}"
        );
    }
}
