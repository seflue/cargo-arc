//! SVG Generation

use crate::layout::{ItemKind, LayoutIR, NodeId};
use std::collections::{HashMap, HashSet};

mod constants;
mod css;
mod elements;
mod positioning;
mod static_data;
pub use constants::RenderConfig;
use css::render_styles;
use elements::{
    render_edges, render_header, render_nodes, render_sidebar, render_toolbar, render_tree_lines,
};
use positioning::{
    PositionedItem, calculate_box_width, calculate_canvas_size, calculate_max_arc_width,
    calculate_positions, collapse_positions, item_nesting,
};
use static_data::render_script;

/// Render `LayoutIR` to SVG string
#[must_use]
pub fn render(ir: &LayoutIR, config: &RenderConfig) -> String {
    let box_width = calculate_box_width(ir);
    // Full positions for STATIC_DATA (JS needs all positions for expand/collapse)
    let positioned_all = calculate_positions(ir, config, box_width);

    // Collect all node IDs that are parents (have children)
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

    // When expand_level is set, compute visibility and collapsed positions
    let (visible_nodes, collapsed_parents) = match config.expand_level {
        Some(level) => {
            let visible: HashSet<NodeId> = ir
                .items
                .iter()
                .filter(|item| item_nesting(&item.kind) <= level)
                .map(|item| item.id)
                .collect();
            let collapsed: HashSet<NodeId> = parents
                .iter()
                .copied()
                .filter(|&id| {
                    let item = &ir.items[id];
                    item_nesting(&item.kind) >= level
                })
                .collect();
            (Some(visible), collapsed)
        }
        None => (None, HashSet::new()),
    };

    // Positions for SVG rendering: collapsed or full
    let positioned_visible = match &visible_nodes {
        Some(visible) => collapse_positions(&positioned_all, visible, config),
        None => positioned_all.clone(),
    };

    let positioned_vis_index: HashMap<NodeId, &PositionedItem> =
        positioned_visible.iter().map(|p| (p.id, p)).collect();
    let max_arc_width = calculate_max_arc_width(&positioned_vis_index, ir, config.row_height);
    let (width, height) = calculate_canvas_size(&positioned_visible, config, max_arc_width);

    let mut svg = String::new();
    svg.push_str(&render_header(width, height));
    svg.push_str(&render_styles());
    svg.push_str("  <g id=\"graph-content\">\n");
    svg.push_str(&render_tree_lines(&positioned_vis_index, ir));
    svg.push_str(&render_nodes(
        &positioned_all,
        &parents,
        visible_nodes.as_ref(),
        &collapsed_parents,
        &positioned_vis_index,
    ));
    svg.push_str(&render_edges(
        &positioned_vis_index,
        ir,
        config.row_height,
        visible_nodes.as_ref(),
    ));
    svg.push_str("  </g>\n");
    let has_externals = ir
        .items
        .iter()
        .any(|item| matches!(item.kind, ItemKind::ExternalSection));
    let has_transitive_externals = ir.items.iter().any(|item| {
        matches!(
            item.kind,
            ItemKind::ExternalCrate {
                is_direct_dependency: false,
                ..
            }
        )
    });
    let initial_collapsed = !collapsed_parents.is_empty();
    svg.push_str(&render_toolbar(
        width,
        has_externals,
        has_transitive_externals,
        initial_collapsed,
    ));
    svg.push_str(&render_sidebar(width));
    svg.push_str(&render_script(config, ir, &positioned_all, &parents));
    svg.push_str("</svg>\n");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{CycleKind, LayoutEdge};
    use crate::model::EdgeContext;

    #[test]
    fn test_render_expand_level_zero() {
        let mut ir = LayoutIR::new();
        let c = ir.add_item(ItemKind::Crate, "my_crate".into());
        let a = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "mod_a".into(),
        );
        let b = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "mod_b".into(),
        );
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));

        let config = RenderConfig {
            expand_level: Some(0),
            ..RenderConfig::default()
        };
        let svg = render(&ir, &config);

        // Modules should have collapsed class
        assert!(
            svg.contains(r#"class="module collapsed""#),
            "Modules should have collapsed class with expand_level=0"
        );
        // No edges should be rendered (all between hidden modules)
        assert!(
            !svg.contains(r#"class="dep-arc"#),
            "No edges should appear with expand_level=0"
        );
        // STATIC_DATA should contain expandLevel
        assert!(
            svg.contains(r#""expandLevel":0"#),
            "STATIC_DATA should contain expandLevel"
        );
        // STATIC_DATA should contain nesting
        assert!(
            svg.contains(r#""nesting":0"#),
            "STATIC_DATA should contain nesting for crate"
        );
        assert!(
            svg.contains(r#""nesting":1"#),
            "STATIC_DATA should contain nesting for module"
        );
        // Toolbar should say "Expand All"
        assert!(
            svg.contains("Expand All"),
            "Toolbar should show 'Expand All' with expand_level=0"
        );
    }

    #[test]
    fn test_render_expand_level_none_unchanged() {
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
        // No collapsed class without expand_level
        assert!(
            !svg.contains(r#"class="module collapsed""#),
            "No collapsed class without expand_level"
        );
        assert!(
            svg.contains("Collapse All"),
            "Toolbar should show 'Collapse All' without expand_level"
        );
    }

    #[test]
    fn test_render_empty() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn test_svg_root_has_cluster_mode_on_by_default() {
        let ir = LayoutIR::new();
        let svg = render(&ir, &RenderConfig::default());
        assert!(
            svg.contains(&format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"{}\"",
                crate::render::constants::CSS.relation.cluster_mode_on
            )),
            "SVG root should carry the cluster-mode-on state class by default"
        );
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                CycleKind::Direct,
                vec![0],
                0,
            ));
        let svg = render(&ir, &RenderConfig::default());
        assert!(svg.contains("cycle-arc"));
        // DirectCycle should have two arrows (bidirectional). cycle-arrow is an
        // additive marker on top of the directional arrow class.
        assert_eq!(svg.matches(r#"class="dep-arrow cycle-arrow""#).count(), 2);

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
        ir2.edges.push(
            LayoutEdge::new(a2, b2, EdgeContext::production()).with_cycle(
                CycleKind::Transitive,
                vec![0],
                0,
            ),
        );
        let svg2 = render(&ir2, &RenderConfig::default());
        assert!(svg2.contains("cycle-arc"));
        // Transitive cycle arcs use solid lines (no dasharray) for uniform style
        // Check only the cycle arc element, not the entire SVG (CSS may contain dasharray for other rules)
        let cycle_start = svg2.find("cycle-arc").expect("cycle-arc should exist");
        let cycle_section = &svg2[cycle_start..cycle_start + 200];
        assert!(
            !cycle_section.contains("stroke-dasharray"),
            "Transitive cycle arc element should NOT have stroke-dasharray, got: {cycle_section}"
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));
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
    fn test_arc_z_order() {
        use crate::model::TestKind;

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
        let d = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "d".into(),
        );
        let e = ir.add_item(
            ItemKind::Module {
                nesting: 1,
                parent: c,
            },
            "e".into(),
        );

        // Add edges in "wrong" order: cycle first, then production, then test
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()).with_cycle(
                CycleKind::Direct,
                vec![0],
                0,
            ));
        ir.edges
            .push(LayoutEdge::new(b, d, EdgeContext::production()));
        ir.edges
            .push(LayoutEdge::new(d, e, EdgeContext::test(TestKind::Unit)));

        let svg = render(&ir, &RenderConfig::default());

        // In SVG, elements rendered later appear on top (higher z-order).
        // Expected order: Test arcs first (back), then Production, then Cycle (front).
        let test_arc_pos = svg.find(r#"id="edge-3-4""#).expect("test arc should exist");
        let prod_arc_pos = svg
            .find(r#"id="edge-2-3""#)
            .expect("production arc should exist");
        let cycle_arc_pos = svg
            .find(r#"id="edge-1-2""#)
            .expect("cycle arc should exist");

        assert!(
            test_arc_pos < prod_arc_pos,
            "Test arc (pos {test_arc_pos}) should appear before production arc (pos {prod_arc_pos}) in SVG"
        );
        assert!(
            prod_arc_pos < cycle_arc_pos,
            "Production arc (pos {prod_arc_pos}) should appear before cycle arc (pos {cycle_arc_pos}) in SVG"
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
        ir.edges
            .push(LayoutEdge::new(a, b, EdgeContext::production()));
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
