use super::constants::{LAYOUT, RenderConfig};
use crate::layout::{ItemKind, LayoutIR, NodeId};

/// Calculate text width based on character count
fn calculate_text_width(text: &str) -> f32 {
    text.len() as f32 * LAYOUT.char_width
}

/// Calculate uniform box width from longest label in LayoutIR
pub(super) fn calculate_box_width(ir: &LayoutIR) -> f32 {
    ir.items
        .iter()
        .map(|item| calculate_text_width(&item.label))
        .fold(0.0_f32, |a, b| a.max(b))
        + LAYOUT.box_padding
}

/// Calculate maximum arc width from edges
pub(super) fn calculate_max_arc_width(
    positioned: &[PositionedItem],
    ir: &LayoutIR,
    row_height: f32,
) -> f32 {
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

/// Positioned item for rendering
pub(super) struct PositionedItem {
    pub id: NodeId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
    pub kind: ItemKind,
}

pub(super) fn calculate_positions(
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

pub(super) fn calculate_canvas_size(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{EdgeKind, ItemKind, LayoutIR};

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
        ir.add_edge(1, 10, EdgeKind::Downward, vec![], false);

        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let max_arc_width = calculate_max_arc_width(&positioned, &ir, config.row_height);
        let (width, _height) = calculate_canvas_size(&positioned, &config, max_arc_width);

        // max_arc for 9 hops = 20 + 9*15 + 8 = 163px
        // Canvas must include box_width + max_arc + margin
        let expected_min = box_width + 163.0;
        assert!(
            width >= expected_min,
            "canvas width {} should be >= {} (box_width {} + arc 163)",
            width,
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

        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);

        let widths: Vec<f32> = positioned.iter().map(|p| p.width).collect();
        assert_eq!(widths.len(), 2, "Expected 2 positioned items");
        assert_eq!(
            widths[0], widths[1],
            "All boxes should have same width: {:?}",
            widths
        );
    }

    #[test]
    fn test_canvas_includes_toolbar_height() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "test".into());
        let config = RenderConfig::default();
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let max_arc_width = calculate_max_arc_width(&positioned, &ir, config.row_height);
        let (_width, height) = calculate_canvas_size(&positioned, &config, max_arc_width);

        // Height should include LAYOUT.toolbar.height (40) + margin (20*2) + 1 row (30) + shadow padding
        assert!(
            height >= LAYOUT.toolbar.height + config.margin * 2.0 + config.row_height,
            "Canvas height {} should include toolbar height {}",
            height,
            LAYOUT.toolbar.height
        );
    }
}
