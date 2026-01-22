//! SVG Generation

use crate::layout::{EdgeKind, ItemKind, LayoutIR, NodeId};

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

    let mut svg = String::new();
    svg.push_str(&render_header(width, height));
    svg.push_str(&render_styles());
    svg.push_str(&render_tree_lines(&positioned, ir));
    svg.push_str(&render_nodes(&positioned));
    svg.push_str(&render_edges(&positioned, ir));
    svg.push_str(&render_script());
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
                y: config.margin + (index as f32 * config.row_height),
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
    let height = if positioned.is_empty() {
        config.margin * 2.0
    } else {
        config.margin * 2.0 + positioned.len() as f32 * config.row_height
    };
    // Width: max(box_right_edge) + arc_space + margin
    let max_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, |a, b| a.max(b));
    // Use calculated max_arc_width, with a minimum buffer for short/no edges
    let arc_space = max_arc_width.max(50.0);
    let width = max_x + arc_space + config.margin;
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
    .crate { fill: #e3f2fd; stroke: #1976d2; stroke-width: 2; }
    .module { fill: #fff3e0; stroke: #f57c00; stroke-width: 1; }
    .label { font-family: monospace; font-size: 12px; }
    .tree-line { stroke: #666; stroke-width: 1; }
    .dep-arc { fill: none; stroke: #9c27b0; stroke-width: 1.5; }
    .dep-arrow { fill: #9c27b0; }
    .cycle-arc { fill: none; stroke: #f44336; stroke-width: 1.5; }
    .cycle-arrow { fill: #f44336; }
    /* Interactive highlighting */
    .highlighted { stroke: #ff9800 !important; stroke-width: 3 !important; }
    .highlighted-node { fill: #fff176 !important; stroke: #ff9800 !important; stroke-width: 3 !important; }
    .highlighted-arrow { fill: #ff9800 !important; }
    .dimmed { opacity: 0.3; }
    .crate, .module, .dep-arc, .cycle-arc { cursor: pointer; }
  </style>
"#
    .to_string()
}

fn render_script() -> String {
    r#"  <script><![CDATA[
(function() {
  function clearHighlights() {
    document.querySelectorAll('.highlighted, .highlighted-node, .highlighted-arrow, .dimmed')
      .forEach(el => el.classList.remove('highlighted', 'highlighted-node', 'highlighted-arrow', 'dimmed'));
  }

  function highlightEdge(from, to) {
    clearHighlights();
    document.getElementById('node-' + from)?.classList.add('highlighted-node');
    document.getElementById('node-' + to)?.classList.add('highlighted-node');
    document.getElementById('edge-' + from + '-' + to)?.classList.add('highlighted');
    document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
      .forEach(el => el.classList.add('highlighted-arrow'));
    document.querySelectorAll('rect:not(.highlighted-node), path:not(.highlighted), polygon:not(.highlighted-arrow)')
      .forEach(el => el.classList.add('dimmed'));
  }

  function highlightNode(nodeId) {
    clearHighlights();
    document.getElementById('node-' + nodeId)?.classList.add('highlighted-node');
    document.querySelectorAll('[data-from="' + nodeId + '"], [data-to="' + nodeId + '"]')
      .forEach(edge => {
        edge.classList.add('highlighted');
        const from = edge.dataset.from;
        const to = edge.dataset.to;
        document.getElementById('node-' + from)?.classList.add('highlighted-node');
        document.getElementById('node-' + to)?.classList.add('highlighted-node');
        document.querySelectorAll('[data-edge="' + from + '-' + to + '"]')
          .forEach(arr => arr.classList.add('highlighted-arrow'));
      });
    document.querySelectorAll('rect:not(.highlighted-node), path:not(.highlighted), polygon:not(.highlighted-arrow)')
      .forEach(el => el.classList.add('dimmed'));
  }

  document.querySelectorAll('.crate, .module').forEach(node => {
    node.addEventListener('click', e => {
      e.stopPropagation();
      highlightNode(node.id.replace('node-', ''));
    });
  });

  document.querySelectorAll('.dep-arc, .cycle-arc').forEach(arc => {
    arc.addEventListener('click', e => {
      e.stopPropagation();
      highlightEdge(arc.dataset.from, arc.dataset.to);
    });
  });

  document.querySelector('svg').addEventListener('click', clearHighlights);
})();
]]></script>
"#
    .to_string()
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
                // Vertical line from parent
                let line_x = parent_pos.x + 10.0;
                let parent_bottom = parent_pos.y + parent_pos.height;
                let child_mid_y = child_pos.y + child_pos.height / 2.0;

                // Vertical segment
                lines.push_str(&format!(
                    "    <line class=\"tree-line\" x1=\"{line_x}\" y1=\"{parent_bottom}\" x2=\"{line_x}\" y2=\"{child_mid_y}\"/>\n"
                ));

                // Horizontal segment to child
                let child_left = child_pos.x;
                lines.push_str(&format!(
                    "    <line class=\"tree-line\" x1=\"{line_x}\" y1=\"{child_mid_y}\" x2=\"{child_left}\" y2=\"{child_mid_y}\"/>\n"
                ));
            }
        }
    }

    lines.push_str("  </g>\n");
    lines
}

fn render_nodes(positioned: &[PositionedItem]) -> String {
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
        let label = escape_xml(&item.label);
        let text_x = item.x + 10.0;
        let text_y = item.y + item.height / 2.0 + 4.0;

        nodes.push_str(&format!(
            "    <rect class=\"{class}\" id=\"node-{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{rx}\"/>\n",
            item.id, item.x, item.y, item.width, item.height
        ));
        nodes.push_str(&format!(
            "    <text class=\"label\" x=\"{text_x}\" y=\"{text_y}\">{label}</text>\n"
        ));
    }

    nodes.push_str("  </g>\n");
    nodes
}

fn render_edges(positioned: &[PositionedItem], ir: &LayoutIR) -> String {
    let mut edges = String::new();
    edges.push_str("  <g id=\"dependencies\">\n");

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

            let (arc_class, arrow_class, extra_style) = match edge.kind {
                EdgeKind::Normal => ("dep-arc", "dep-arrow", ""),
                EdgeKind::DirectCycle => ("cycle-arc", "cycle-arrow", ""),
                EdgeKind::TransitiveCycle => {
                    ("cycle-arc", "cycle-arrow", " stroke-dasharray=\"4,2\"")
                }
            };

            let edge_id = format!("{}-{}", edge.from, edge.to);
            edges.push_str(&format!(
                "    <path class=\"{arc_class}\" id=\"edge-{edge_id}\" data-from=\"{}\" data-to=\"{}\" d=\"{path}\"{extra_style}/>\n",
                edge.from, edge.to
            ));

            // Arrow head pointing to target
            let arrow = render_arrow(to_x, to_y, arrow_class, &edge_id);
            edges.push_str(&arrow);

            // For DirectCycle, add reverse arrow
            if edge.kind == EdgeKind::DirectCycle {
                let reverse_arrow = render_arrow(from_x, from_y, arrow_class, &edge_id);
                edges.push_str(&reverse_arrow);
            }
        }
    }

    edges.push_str("  </g>\n");
    edges
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
        ir.add_edge(a, b, EdgeKind::Normal);
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
        ir.add_edge(a, b, EdgeKind::DirectCycle);
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
        ir2.add_edge(a2, b2, EdgeKind::TransitiveCycle);
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
        ir.add_edge(1, 10, EdgeKind::Normal);

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
        // Extract all rect widths
        let widths: Vec<f32> = svg
            .lines()
            .filter(|l| l.contains("<rect"))
            .filter_map(|l| {
                let start = l.find("width=\"")? + 7;
                let rest = &l[start..];
                let end = rest.find('"')?;
                rest[..end].parse().ok()
            })
            .collect();

        assert_eq!(widths.len(), 2, "Expected 2 rects");
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
        ir.add_edge(a, b, EdgeKind::Normal);
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains(r#"id="edge-1-2""#), "Edge should have id");
        assert!(
            svg.contains(r#"data-from="1""#),
            "Edge should have data-from"
        );
        assert!(svg.contains(r#"data-to="2""#), "Edge should have data-to");
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
}
