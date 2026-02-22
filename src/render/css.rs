use super::constants::{
    BLUE, BLUE_100, BLUE_300, COLORS, CSS, GRAY_50, GRAY_200, GRAY_400, GRAY_600, GREEN, LAYOUT,
    ORANGE, ORANGE_100, ORANGE_300, PURPLE,
};

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
        Self::new(&format!(".{}", name), properties)
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
            c.nodes.label,
            &[
                ("font-family", "monospace"),
                ("font-size", "12px"),
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
        CssRule::class(c.direction.dep_arrow, &[("fill", d.downward)]),
        CssRule::class(c.direction.upward_arrow, &[("fill", d.upward)]),
        CssRule::class(
            c.direction.cycle_arc,
            &[
                ("fill", "none"),
                ("stroke", d.cycle),
                ("stroke-width", "1.0"),
            ],
        ),
        CssRule::class(c.direction.cycle_arrow, &[("fill", d.cycle)]),
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
        CssRule::class(c.relation.glow_incoming, &[("stroke", r.dependency)]),
        CssRule::class(c.relation.glow_outgoing, &[("stroke", r.dependent)]),
        // Node borders (relation)
        CssRule::class(
            c.relation.dep_node,
            &[("stroke", r.dependency), ("stroke-width", "2.5")],
        ),
        CssRule::class(
            c.relation.dependent_node,
            &[("stroke", r.dependent), ("stroke-width", "2.5")],
        ),
        // Dimmed
        CssRule::class(
            c.relation.dimmed,
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
        CssRule::new(
            &format!(
                "polygon.{}.{}",
                c.direction.virtual_arrow, c.relation.dimmed
            ),
            &[("fill", r.dimmed)],
        ),
        // CSS-only dimming via has-highlight on SVG root (leaf elements only)
        CssRule::new(
            &format!(
                "svg.{} rect:not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{}):not(.{})",
                c.relation.has_highlight,
                c.node_selection.selected_crate,
                c.node_selection.selected_module,
                c.node_selection.group_member,
                c.node_selection.cycle_member,
                c.relation.dep_node,
                c.relation.dependent_node,
                c.toolbar.btn,
                c.labels.arc_count_bg
            ),
            &[("opacity", "0.3"), ("pointer-events", "none")],
        ),
        CssRule::new(
            &format!(
                "svg.{} path:not(.{}):not(.{}):not(.{}):not(.{})",
                c.relation.has_highlight,
                c.relation.highlighted_arc,
                c.direction.arc_hitarea,
                c.direction.virtual_hitarea,
                c.relation.shadow_path
            ),
            &[
                ("opacity", "0.3"),
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
                ("opacity", "0.3"),
                ("pointer-events", "none"),
                ("fill", r.dimmed),
            ],
        ),
        CssRule::new(
            &format!(
                "svg.{} text.{}:not(.{})",
                c.relation.has_highlight, c.labels.arc_count, c.relation.highlighted_label
            ),
            &[("opacity", "0.3"), ("fill", r.dimmed)],
        ),
        CssRule::new(
            &format!("svg.{} line", c.relation.has_highlight),
            &[("opacity", "0.3"), ("pointer-events", "none")],
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
                ".{}, .{}, .{}, .{}",
                c.nodes.crate_node, c.nodes.module, c.direction.dep_arc, c.direction.cycle_arc
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
        // Virtual arcs
        CssRule::class(
            c.direction.virtual_arc,
            &[("fill", "none"), ("stroke-width", "0.5")],
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
            &[("opacity", "0.3"), ("fill", r.dimmed)],
        ),
        CssRule::class(
            c.nodes.child_count,
            &[("font-size", "10px"), ("fill", n.child_count)],
        ),
        // Shadow path
        CssRule::class(
            c.relation.shadow_path,
            &[("pointer-events", "none"), ("stroke-linecap", "round")],
        ),
        // Toolbar
        CssRule::class(c.toolbar.view_options, &[("cursor", "default")]),
        CssRule::class(
            c.toolbar.btn,
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
        CssRule::class(
            c.toolbar.btn_text,
            &[
                ("font-family", "sans-serif"),
                ("font-size", "11px"),
                ("text-anchor", "middle"),
            ],
        ),
        CssRule::class(
            c.toolbar.checkbox,
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
        CssRule::class(
            c.toolbar.label,
            &[
                ("font-family", "sans-serif"),
                ("font-size", "11px"),
                ("cursor", "pointer"),
            ],
        ),
        CssRule::class(c.toolbar.separator, &[("stroke", t.separator)]),
        CssRule::class(
            c.toolbar.disabled,
            &[("opacity", "0.4"), ("pointer-events", "none")],
        ),
        // Filter visibility
        CssRule::class(c.labels.hidden_by_filter, &[("display", "none")]),
        // Sidebar
        CssRule::class(
            c.sidebar.root,
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
        CssRule::class(
            c.sidebar.header,
            &[
                ("display", "flex"),
                ("justify-content", "space-between"),
                ("align-items", "center"),
                ("padding", "8px 10px"),
                ("border-bottom", &format!("1px solid {}", GRAY_200)),
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
        CssRule::class(
            c.sidebar.arrow,
            &[
                ("color", GRAY_400),
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
        CssRule::class(
            c.sidebar.content,
            &[
                ("overflow-y", "auto"),
                ("padding", "8px 10px"),
                ("flex", "1"),
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
                ("color", GRAY_400),
                ("padding-left", "12px"),
                ("font-size", "11px"),
                ("white-space", "nowrap"),
            ],
        ),
        CssRule::class(
            c.sidebar.toggle,
            &[
                ("font-size", "10px"),
                ("color", GRAY_400),
                ("width", "12px"),
            ],
        ),
        CssRule::class(c.sidebar.symbol_name, &[("font-weight", "bold")]),
        CssRule::class(c.sidebar.ns, &[("color", GRAY_400), ("font-size", "10px")]),
        CssRule::class(
            c.sidebar.ref_count,
            &[
                ("color", GRAY_400),
                ("font-size", "10px"),
                ("margin-left", "auto"),
            ],
        ),
        CssRule::class(c.sidebar.locations, &[("padding-left", "16px")]),
        CssRule::class(
            c.sidebar.line_badge,
            &[
                ("background", BLUE_100),
                ("color", BLUE),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
                ("font-size", "10px"),
            ],
        ),
        CssRule::class(
            c.sidebar.divider,
            &[
                ("border", "none"),
                ("border-top", &format!("1px solid {}", GRAY_200)),
                ("margin", "6px 0"),
            ],
        ),
        CssRule::class(
            c.sidebar.footer,
            &[
                ("padding", "6px 10px"),
                ("border-top", &format!("1px solid {}", GRAY_200)),
                ("font-size", "10px"),
                ("color", GRAY_400),
            ],
        ),
        CssRule::class(
            c.sidebar.node_crate,
            &[
                ("background", BLUE_100),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::class(
            c.sidebar.node_module,
            &[
                ("background", ORANGE_100),
                ("padding", "1px 4px"),
                ("border-radius", "3px"),
            ],
        ),
        CssRule::class(
            c.sidebar.node_from,
            &[("border", &format!("2px solid {}", PURPLE))],
        ),
        CssRule::class(
            c.sidebar.node_to,
            &[("border", &format!("2px solid {}", GREEN))],
        ),
        CssRule::new(
            &format!(".{}.{}", c.sidebar.node_crate, c.sidebar.node_selected),
            &[
                ("background", BLUE_300),
                ("border", &format!("2px solid {}", BLUE)),
            ],
        ),
        CssRule::new(
            &format!(".{}.{}", c.sidebar.node_module, c.sidebar.node_selected),
            &[
                ("background", ORANGE_300),
                ("border", &format!("2px solid {}", ORANGE)),
            ],
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
    ]
}

pub(super) fn render_styles() -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
            "rect dimming rule should exclude .group-member, got: {}",
            rect_dim_section
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
                CSS.node_selection.cycle_member, COLORS.direction.cycle
            )),
            "cycle-member should use cycle color for stroke"
        );
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
            css.contains(".arc-count"),
            "CSS should contain .arc-count style"
        );
        assert!(
            css.contains(".child-count"),
            "CSS should contain .child-count style"
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
}
