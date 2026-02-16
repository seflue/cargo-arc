use super::constants::{COLORS, CSS, LAYOUT};
use super::positioning::PositionedItem;
use crate::layout::{CycleKind, EdgeDirection, ItemKind, LayoutIR, NodeId};
use crate::model::DependencyKind;
use std::collections::HashSet;

pub(super) fn render_header(width: f32, height: f32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
"#
    )
}

pub(super) fn render_sidebar(width: f32) -> String {
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

pub(super) fn render_toolbar(width: f32) -> String {
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

pub(super) fn render_tree_lines(positioned: &[PositionedItem], ir: &LayoutIR) -> String {
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

pub(super) fn render_nodes(positioned: &[PositionedItem], parents: &HashSet<NodeId>) -> String {
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

pub(super) fn render_edges(
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
            let (base_arc_class, arrow_class, direction) = match (edge.cycle, edge.direction) {
                (Some(_), _) => (cd.cycle_arc.to_string(), cd.cycle_arrow, "cycle"),
                (None, EdgeDirection::Downward) => (
                    format!("{} {}", cd.dep_arc, cd.downward),
                    cd.dep_arrow,
                    "downward",
                ),
                (None, EdgeDirection::Upward) => (
                    format!("{} {}", cd.dep_arc, cd.upward),
                    cd.upward_arrow,
                    "upward",
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
            let cycle_ids_attr = if edge.cycle_ids.is_empty() {
                String::new()
            } else {
                let ids: Vec<String> = edge.cycle_ids.iter().map(|id| id.to_string()).collect();
                format!(r#" data-cycle-ids="{}""#, ids.join(","))
            };

            // Hit-area path (invisible, 12px wide, receives pointer events) → hitareas layer
            // Note: source-locations are read from STATIC_DATA in JavaScript, not DOM attributes
            let hitarea = cd.arc_hitarea;
            hitareas.push_str(&format!(
                "    <path class=\"{hitarea}\" data-arc-id=\"{edge_id}\" data-from=\"{}\" data-to=\"{}\" data-direction=\"{direction}\"{cycle_ids_attr} d=\"{path}\"/>\n",
                edge.from, edge.to
            ));
            // Visible path (styled, no pointer events) → base-arcs layer
            base_arcs.push_str(&format!(
                "    <path class=\"{arc_class}\" id=\"edge-{edge_id}\" data-arc-id=\"{edge_id}\" data-direction=\"{direction}\"{cycle_ids_attr} d=\"{path}\"/>\n"
            ));

            // Arrow head pointing to target → base-arcs layer
            let arrow = render_arrow(to_x, to_y, arrow_class, &edge_id);
            base_arcs.push_str(&arrow);

            // For DirectCycle, add reverse arrow
            if edge.cycle == Some(CycleKind::Direct) {
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
    use super::super::constants::RenderConfig;
    use super::super::positioning::{calculate_box_width, calculate_positions};
    use super::*;
    use crate::model::EdgeContext;

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
        let output = render_tree_lines(&positioned, &ir);
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
        let output = render_tree_lines(&positioned, &ir);
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
        let output = render_nodes(&positioned, &parents);
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
        let output = render_nodes(&positioned, &parents);
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
        let output = render_nodes(&positioned, &parents);
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
        let output = render_nodes(&positioned, &parents);
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
        let output = render_nodes(&positioned, &parents);
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
    fn test_render_toolbar_contains_elements() {
        let output = render_toolbar(800.0);
        assert!(
            output.contains(r#"class="view-options""#),
            "Should have view-options group"
        );
        assert!(
            output.contains(r#"id="collapse-toggle-btn""#),
            "Should have collapse toggle button"
        );
        assert!(
            output.contains(r#"id="collapse-toggle-label""#),
            "Should have collapse toggle label"
        );
        assert!(
            output.contains("Collapse All"),
            "Should have 'Collapse All' text"
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
            output.contains(CSS.toolbar.disabled),
            "Should have disabled Tests checkbox"
        );
        assert!(output.contains("Tests"), "Should have 'Tests' label");
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
        ir.add_edge(
            a,
            b,
            EdgeDirection::Downward,
            None,
            vec![],
            vec![],
            EdgeContext::production(),
        );
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);

        let output = render_edges(&positioned, &ir, config.row_height);
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
        ir.add_edge(
            a,
            b,
            EdgeDirection::Downward,
            None,
            vec![],
            vec![],
            EdgeContext::production(),
        );
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);

        let output = render_edges(&positioned, &ir, config.row_height);

        assert!(
            output.contains(r#"class="arc-hitarea""#),
            "Should have hit-area path"
        );
        assert!(
            output.contains(r#"class="dep-arc downward""#),
            "Should have visible dep-arc path with direction"
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
        ir.add_edge(
            c1,
            c2,
            EdgeDirection::Downward,
            None,
            vec![],
            vec![],
            EdgeContext::production(),
        );
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);

        let output = render_edges(&positioned, &ir, config.row_height);
        assert!(
            output.contains("crate-dep-arc"),
            "Crate-to-crate edges should have crate-dep-arc class"
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
        ir.add_edge(
            a,
            b,
            EdgeDirection::Downward,
            Some(CycleKind::Direct),
            vec![0],
            vec![],
            EdgeContext::production(),
        );
        // Non-cycle edge
        ir.add_edge(
            a,
            m,
            EdgeDirection::Downward,
            None,
            vec![],
            vec![],
            EdgeContext::production(),
        );

        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);

        let output = render_edges(&positioned, &ir, config.row_height);

        // Cycle arc path should have data-cycle-ids="0"
        let cycle_path = output
            .lines()
            .find(|l| l.contains("cycle-arc") && l.contains("id=\"edge-1-2\""))
            .expect("Should find cycle-arc path for edge 1-2");
        assert!(
            cycle_path.contains(r#"data-cycle-ids="0""#),
            "Cycle arc path should have data-cycle-ids attribute, got: {}",
            cycle_path
        );

        // Hitarea for cycle arc should also have data-cycle-ids
        let cycle_hitarea = output
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains(r#"data-arc-id="1-2""#))
            .expect("Should find hitarea for edge 1-2");
        assert!(
            cycle_hitarea.contains(r#"data-cycle-ids="0""#),
            "Cycle arc hitarea should have data-cycle-ids attribute, got: {}",
            cycle_hitarea
        );

        // Non-cycle arc should NOT have data-cycle-ids
        let normal_path = output
            .lines()
            .find(|l| l.contains("id=\"edge-1-3\""))
            .expect("Should find normal arc path for edge 1-3");
        assert!(
            !normal_path.contains("data-cycle-ids"),
            "Non-cycle arc should NOT have data-cycle-ids, got: {}",
            normal_path
        );

        // Non-cycle hitarea should NOT have data-cycle-ids
        let normal_hitarea = output
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains(r#"data-arc-id="1-3""#))
            .expect("Should find hitarea for edge 1-3");
        assert!(
            !normal_hitarea.contains("data-cycle-ids"),
            "Non-cycle hitarea should NOT have data-cycle-ids, got: {}",
            normal_hitarea
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
        ir.add_edge(
            a,
            b,
            EdgeDirection::Downward,
            Some(CycleKind::Direct),
            vec![0, 2],
            vec![],
            EdgeContext::production(),
        );

        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let output = render_edges(&positioned, &ir, config.row_height);

        // Visible path should have comma-separated cycle IDs
        let cycle_path = output
            .lines()
            .find(|l| l.contains("cycle-arc") && l.contains("id=\"edge-1-2\""))
            .expect("Should find cycle-arc path for edge 1-2");
        assert!(
            cycle_path.contains(r#"data-cycle-ids="0,2""#),
            "Multi-cycle arc should have comma-separated data-cycle-ids, got: {}",
            cycle_path
        );

        // Hitarea should also have comma-separated cycle IDs
        let hitarea = output
            .lines()
            .find(|l| l.contains("arc-hitarea") && l.contains(r#"data-arc-id="1-2""#))
            .expect("Should find hitarea for edge 1-2");
        assert!(
            hitarea.contains(r#"data-cycle-ids="0,2""#),
            "Multi-cycle hitarea should have comma-separated data-cycle-ids, got: {}",
            hitarea
        );
    }
}
