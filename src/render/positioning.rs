use super::constants::{LAYOUT, RenderConfig};
use crate::layout::{ItemKind, LayoutIR, NodeId};
use std::collections::{HashMap, HashSet};

/// Calculate text width based on character count
#[allow(clippy::cast_precision_loss)] // label lengths stay well below 2^23
fn calculate_text_width(text: &str) -> f32 {
    text.len() as f32 * LAYOUT.char_width
}

/// Calculate uniform box width from longest label in `LayoutIR`
pub(super) fn calculate_box_width(ir: &LayoutIR) -> f32 {
    ir.items
        .iter()
        .map(|item| calculate_text_width(&item.label))
        .fold(0.0_f32, f32::max)
        + LAYOUT.box_padding
        + LAYOUT.toggle_space
        + level_space(ir)
}

/// The room every row keeps for a level number, between the longest label
/// and the collapse symbol: the digits of the highest level and the gap
/// after them. No room when no item has a level.
#[allow(clippy::cast_precision_loss)] // a level has a handful of digits
fn level_space(ir: &LayoutIR) -> f32 {
    ir.items
        .iter()
        .filter_map(|item| item.level)
        .max()
        .map_or(0.0, |highest| {
            highest.to_string().len() as f32 * LAYOUT.level_char_width + LAYOUT.level_gap
        })
}

/// Calculate the maximum width of the arcs to the right of the rows
pub(super) fn calculate_max_arc_width(
    positioned_index: &HashMap<NodeId, &PositionedItem>,
    ir: &LayoutIR,
    row_height: f32,
) -> f32 {
    ir.edges
        .iter()
        .filter(|edge| !edge.direction.is_upward())
        .filter_map(|edge| {
            let from = positioned_index.get(&edge.from).copied()?;
            let to = positioned_index.get(&edge.to).copied()?;
            let rows_spanned = ((to.y - from.y).abs() / row_height).round().max(1.0);
            Some(LAYOUT.arc_base + rows_spanned * LAYOUT.arc_scale + LAYOUT.arrow_length)
        })
        .fold(0.0_f32, f32::max)
}

/// How far an upward arc over `rows_spanned` rows reaches to the left of
/// the rows.
pub(super) fn upward_arc_reach(rows_spanned: f32) -> f32 {
    (LAYOUT.arc_base + rows_spanned * LAYOUT.arc_scale).min(LAYOUT.upward_arc_max)
}

/// Room to the left of the rows for the upward arcs, measured with every row
/// shown so that collapsing moves no row sideways. The positions depend on
/// this room, so the rows an arc spans come from the item ids, which are the
/// row indices while every row is shown.
#[allow(clippy::cast_precision_loss)] // row distances stay well below 2^23
fn upward_arc_room(ir: &LayoutIR) -> f32 {
    ir.edges
        .iter()
        .filter(|edge| edge.direction.is_upward())
        .map(|edge| {
            let rows_spanned = edge.from.abs_diff(edge.to).max(1) as f32;
            upward_arc_reach(rows_spanned) + LAYOUT.arrow_length
        })
        .fold(0.0_f32, f32::max)
}

/// Positioned item for rendering
#[derive(Clone)]
pub(super) struct PositionedItem {
    pub id: NodeId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
    pub kind: ItemKind,
    pub level: Option<usize>,
}

/// Get the nesting depth for an item kind.
/// Package/ExternalSection = 0, Module = its nesting field, `ExternalPackage` = 1.
#[allow(clippy::cast_possible_truncation)] // nesting u32 fits in usize
pub(super) fn item_nesting(kind: &ItemKind) -> usize {
    match kind {
        ItemKind::Package | ItemKind::ExternalSection => 0,
        ItemKind::Module { nesting, .. } => *nesting as usize,
        ItemKind::ExternalPackage { .. } => 1,
    }
}

/// Build collapsed positions: filter to visible nodes, assign gap-free Y positions.
/// `positioned_all` contains the full layout, `visible` determines which nodes to keep.
#[allow(clippy::cast_precision_loss)] // item count stays well below 2^23
pub(super) fn collapse_positions(
    positioned_all: &[PositionedItem],
    visible: &HashSet<NodeId>,
    config: &RenderConfig,
) -> Vec<PositionedItem> {
    let mut result = Vec::new();
    let mut current_y = config.margin + LAYOUT.toolbar.height;
    for item in positioned_all {
        if !visible.contains(&item.id) {
            continue;
        }
        result.push(PositionedItem {
            id: item.id,
            x: item.x,
            y: current_y,
            width: item.width,
            height: item.height,
            label: item.label.clone(),
            kind: item.kind.clone(),
            level: item.level,
        });
        current_y += config.row_height;
    }
    result
}

#[allow(clippy::cast_precision_loss)] // nesting depth and item count stay well below 2^23
pub(super) fn calculate_positions(
    ir: &LayoutIR,
    config: &RenderConfig,
    box_width: f32,
) -> Vec<PositionedItem> {
    let left = config.margin + upward_arc_room(ir);
    ir.items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let nesting = match &item.kind {
                ItemKind::Package | ItemKind::ExternalSection => 0,
                ItemKind::Module { nesting, .. } => *nesting,
                ItemKind::ExternalPackage { .. } => 1,
            };
            let height = match &item.kind {
                ItemKind::Package | ItemKind::ExternalSection => LAYOUT.package_height,
                ItemKind::Module { .. } | ItemKind::ExternalPackage { .. } => LAYOUT.module_height,
            };
            PositionedItem {
                id: item.id,
                x: left + (nesting as f32 * config.indent_size),
                y: config.margin + LAYOUT.toolbar.height + (index as f32 * config.row_height),
                width: box_width,
                height,
                label: item.label.clone(),
                kind: item.kind.clone(),
                level: item.level,
            }
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)] // item count stays well below 2^23
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
    // Toolbar offset is kept in height (nodes are positioned below toolbar area).
    // Sidebar shadow padding is handled dynamically by JS when the sidebar opens.
    let height = base_height + LAYOUT.toolbar.height;

    // Width: max(box_right_edge) + arc_space + margin
    // Sidebar space is added dynamically by JS when the sidebar opens.
    let max_x = positioned
        .iter()
        .map(|p| p.x + p.width)
        .fold(0.0_f32, f32::max);
    // Use calculated max_arc_width, with a minimum buffer for short/no edges
    let arc_space = max_arc_width.max(LAYOUT.arc_min_space);
    let width = max_x + arc_space + config.margin;
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::rust::PROFILE as RUST_PROFILE;
    use crate::layout::{EdgeDirection, ItemKind, LayoutEdge, LayoutIR};
    use crate::model::EdgeContext;
    use std::collections::HashMap;

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
        ir.add_item(ItemKind::Package, "cli".into());
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
    fn test_box_width_leaves_a_gap_between_longest_label_and_toggle() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Package, "a_long_crate_name".into());

        let width = calculate_box_width(&ir);
        let text_end = LAYOUT.text_padding_x + calculate_text_width("a_long_crate_name");
        let toggle_start = width - LAYOUT.toggle_offset;
        assert!(
            toggle_start - text_end >= 8.0,
            "toggle starts {} px after the label",
            toggle_start - text_end
        );
    }

    #[test]
    fn test_canvas_width_includes_max_arc() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Package, "c".into());
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
        // Edge von erstem zu letztem Modul (9 Zeilen)
        ir.edges
            .push(LayoutEdge::new(1, 10, EdgeContext::production()));

        let config = RenderConfig::new(&RUST_PROFILE.wording);
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();
        let max_arc_width = calculate_max_arc_width(&positioned_index, &ir, config.row_height);
        let (width, _height) = calculate_canvas_size(&positioned, &config, max_arc_width);

        // max_arc for 9 rows = 20 + 9*15 + 8 = 163px
        // Canvas must include box_width + max_arc + margin
        let expected_min = box_width + 163.0;
        assert!(
            width >= expected_min,
            "canvas width {width} should be >= {expected_min} (box_width {box_width} + arc 163)"
        );
    }

    #[test]
    #[allow(clippy::float_cmp)] // every box copies the one box_width, so identity is the claim
    fn test_all_boxes_same_width() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Package, "short".into());
        ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: 0,
            },
            "very_long_module_name".into(),
        );

        let config = RenderConfig::new(&RUST_PROFILE.wording);
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);

        let widths: Vec<f32> = positioned.iter().map(|p| p.width).collect();
        assert_eq!(widths.len(), 2, "Expected 2 positioned items");
        assert_eq!(
            widths[0], widths[1],
            "All boxes should have same width: {widths:?}"
        );
    }

    #[test]
    fn test_collapse_positions_hides_one_of_three() {
        let config = RenderConfig::new(&RUST_PROFILE.wording);
        let items = vec![
            PositionedItem {
                id: 0,
                x: 20.0,
                y: 60.0,
                width: 100.0,
                height: 24.0,
                label: "crate".into(),
                kind: ItemKind::Package,
                level: None,
            },
            PositionedItem {
                id: 1,
                x: 40.0,
                y: 90.0,
                width: 100.0,
                height: 20.0,
                label: "mod_a".into(),
                kind: ItemKind::Module {
                    nesting: 1,
                    parent: 0,
                },
                level: None,
            },
            PositionedItem {
                id: 2,
                x: 40.0,
                y: 120.0,
                width: 100.0,
                height: 20.0,
                label: "mod_b".into(),
                kind: ItemKind::Module {
                    nesting: 1,
                    parent: 0,
                },
                level: None,
            },
        ];

        let mut visible: HashSet<NodeId> = HashSet::new();
        visible.insert(0);
        visible.insert(2); // hide item 1

        let result = collapse_positions(&items, &visible, &config);

        assert_eq!(result.len(), 2, "should contain 2 visible items");
        assert_eq!(result[0].id, 0);
        assert_eq!(result[1].id, 2);

        // Gap-free Y: second item directly after first
        let expected_y0 = config.margin + LAYOUT.toolbar.height;
        let expected_y1 = expected_y0 + config.row_height;
        assert!(
            (result[0].y - expected_y0).abs() < 0.01,
            "first Y {}, expected {}",
            result[0].y,
            expected_y0
        );
        assert!(
            (result[1].y - expected_y1).abs() < 0.01,
            "second Y {}, expected {} (gap-free)",
            result[1].y,
            expected_y1
        );
    }

    #[test]
    fn test_collapse_positions_empty_visible_set() {
        let config = RenderConfig::new(&RUST_PROFILE.wording);
        let items = vec![PositionedItem {
            id: 0,
            x: 20.0,
            y: 60.0,
            width: 100.0,
            height: 24.0,
            label: "crate".into(),
            kind: ItemKind::Package,
            level: None,
        }];

        let visible: HashSet<NodeId> = HashSet::new();
        let result = collapse_positions(&items, &visible, &config);

        assert!(result.is_empty(), "empty visible set → empty result");
    }

    #[test]
    fn test_item_nesting_variants() {
        assert_eq!(item_nesting(&ItemKind::Package), 0);
        assert_eq!(item_nesting(&ItemKind::ExternalSection), 0);
        assert_eq!(
            item_nesting(&ItemKind::Module {
                nesting: 1,
                parent: 0
            }),
            1
        );
        assert_eq!(
            item_nesting(&ItemKind::Module {
                nesting: 3,
                parent: 0
            }),
            3
        );
        assert_eq!(
            item_nesting(&ItemKind::ExternalPackage {
                parent: 0,
                is_direct_dependency: true
            }),
            1
        );
    }

    #[test]
    fn test_canvas_includes_toolbar_height() {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Package, "test".into());
        let config = RenderConfig::new(&RUST_PROFILE.wording);
        let box_width = calculate_box_width(&ir);
        let positioned = calculate_positions(&ir, &config, box_width);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();
        let max_arc_width = calculate_max_arc_width(&positioned_index, &ir, config.row_height);
        let (_width, height) = calculate_canvas_size(&positioned, &config, max_arc_width);

        // Height should include LAYOUT.toolbar.height (40) + margin (20*2) + 1 row (30)
        assert!(
            height >= LAYOUT.toolbar.height + config.margin * 2.0 + config.row_height,
            "Canvas height {} should include toolbar height {}",
            height,
            LAYOUT.toolbar.height
        );
    }

    /// A package with `modules` child modules on the rows below it.
    fn package_with_modules(modules: usize) -> LayoutIR {
        let mut ir = LayoutIR::new();
        let package = ir.add_item(ItemKind::Package, "c".into());
        for i in 0..modules {
            ir.add_item(
                ItemKind::Module {
                    nesting: 1,
                    parent: package,
                },
                format!("m{i}"),
            );
        }
        ir
    }

    fn upward_edge(from: usize, to: usize) -> LayoutEdge {
        LayoutEdge {
            direction: EdgeDirection::Upward { tie: false },
            ..LayoutEdge::new(from, to, EdgeContext::production())
        }
    }

    #[test]
    fn test_box_width_reserves_room_for_the_level_number() {
        let mut ir = package_with_modules(2);
        let without_levels = calculate_box_width(&ir);
        ir.items[1].level = Some(3);
        ir.items[2].level = Some(12);

        // Two digits of 6 px for the highest level, and a gap of 6.
        assert_eq!(calculate_box_width(&ir), without_levels + 18.0);
    }

    #[test]
    fn test_positioned_item_keeps_its_level() {
        let mut ir = package_with_modules(1);
        ir.items[1].level = Some(3);
        let config = RenderConfig::new(&RUST_PROFILE.wording);

        let positioned = calculate_positions(&ir, &config, 100.0);
        let visible: HashSet<NodeId> = [0, 1].into();
        let collapsed = collapse_positions(&positioned, &visible, &config);

        assert_eq!(positioned[0].level, None);
        assert_eq!(positioned[1].level, Some(3));
        assert_eq!(collapsed[1].level, Some(3));
    }

    #[test]
    fn test_rows_leave_room_on_the_left_for_an_upward_arc() {
        let mut ir = package_with_modules(4);
        // Row 4 uses row 1 against the level order: the arc spans 3 rows.
        ir.edges.push(upward_edge(4, 1));
        let config = RenderConfig::new(&RUST_PROFILE.wording);

        let positioned = calculate_positions(&ir, &config, 100.0);

        // Reach 20 + 3 * 15 = 65, plus the arrow head of 8.
        let room = 73.0;
        assert!((positioned[0].x - (config.margin + room)).abs() < 0.01);
        assert!((positioned[1].x - (config.margin + room + config.indent_size)).abs() < 0.01);
    }

    #[test]
    fn test_rows_start_at_the_margin_without_an_upward_edge() {
        let mut ir = package_with_modules(4);
        ir.edges
            .push(LayoutEdge::new(1, 4, EdgeContext::production()));
        let config = RenderConfig::new(&RUST_PROFILE.wording);

        let positioned = calculate_positions(&ir, &config, 100.0);

        assert!((positioned[0].x - config.margin).abs() < 0.01);
    }

    #[test]
    fn test_room_on_the_left_stops_growing_with_a_long_upward_arc() {
        let mut ir = package_with_modules(60);
        ir.edges.push(upward_edge(60, 1));
        let config = RenderConfig::new(&RUST_PROFILE.wording);

        let positioned = calculate_positions(&ir, &config, 100.0);

        let room = LAYOUT.upward_arc_max + LAYOUT.arrow_length;
        assert!((positioned[0].x - (config.margin + room)).abs() < 0.01);
    }

    #[test]
    fn test_max_arc_width_leaves_out_upward_edges() {
        let mut ir = package_with_modules(10);
        ir.edges.push(upward_edge(10, 1));
        ir.edges
            .push(LayoutEdge::new(1, 2, EdgeContext::production()));
        let config = RenderConfig::new(&RUST_PROFILE.wording);
        let positioned = calculate_positions(&ir, &config, 100.0);
        let positioned_index: HashMap<_, _> = positioned.iter().map(|p| (p.id, p)).collect();

        let max_arc_width = calculate_max_arc_width(&positioned_index, &ir, config.row_height);

        // The one arc on the right spans 1 row: 20 + 15 + 8.
        assert!((max_arc_width - 43.0).abs() < 0.01);
    }
}
