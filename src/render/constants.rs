/// All layout constants consolidated in one place.
/// Use `static` (not `const`) so references like `let tb = &LAYOUT.toolbar` work.
pub(super) struct LayoutConstants {
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
#[allow(clippy::struct_field_names)] // "shadow_" prefix groups related CSS shadow values
pub(super) struct SidebarLayout {
    /// box-shadow Y offset (px)
    pub shadow_offset_y: f32,
    /// box-shadow blur radius (px)
    pub shadow_blur: f32,
    /// box-shadow opacity (0.0–1.0)
    pub shadow_opacity: f32,
}

impl SidebarLayout {
    /// CSS box-shadow value derived from the layout constants.
    #[allow(clippy::cast_possible_truncation)] // layout constants are small integers
    pub fn box_shadow_css(&self) -> String {
        format!(
            "0 {}px {}px rgba(0,0,0,{})",
            self.shadow_offset_y as i32, self.shadow_blur as i32, self.shadow_opacity,
        )
    }

    /// Extra padding needed so SVG canvas and foreignObject don't clip the shadow.
    /// max downward extent = `offset_y` + blur, plus 2px safety margin.
    pub fn shadow_padding(&self) -> f32 {
        self.shadow_offset_y + self.shadow_blur + 2.0
    }
}

#[allow(dead_code)]
pub(super) struct ToolbarLayout {
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

pub(super) static LAYOUT: LayoutConstants = LayoutConstants {
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

pub(super) const GREEN: &str = "#40a02b";
pub(super) const YELLOW: &str = "#df8e1d";
pub(super) const RED: &str = "#d20f39";
pub(super) const PURPLE: &str = "#8839ef";
pub(super) const BLUE: &str = "#1e66f5";
pub(super) const ORANGE: &str = "#fe640b";

pub(super) const BLUE_100: &str = "#dbeafe";
pub(super) const BLUE_300: &str = "#93c5fd";
pub(super) const ORANGE_100: &str = "#ffedd5";
pub(super) const ORANGE_300: &str = "#fdba74";

pub(super) const GRAY_600: &str = "#666";
pub(super) const GRAY_400: &str = "#888";
pub(super) const GRAY_300: &str = "#ccc";
pub(super) const GRAY_200: &str = "#e0e0e0";
pub(super) const GRAY_100: &str = "#f5f5f5";
pub(super) const GRAY_50: &str = "#fafafa";
const WHITE: &str = "#ffffff";

pub(super) struct NodeColors {
    pub crate_fill: &'static str,
    pub crate_stroke: &'static str,
    pub module_fill: &'static str,
    pub module_stroke: &'static str,
    pub external_section_fill: &'static str,
    pub external_section_stroke: &'static str,
    pub external_crate_fill: &'static str,
    pub external_crate_stroke: &'static str,
    pub external_transitive_fill: &'static str,
    pub external_transitive_stroke: &'static str,
    pub tree_line: &'static str,
    pub child_count: &'static str,
    pub collapse_toggle: &'static str,
    pub collapse_hover: &'static str,
}

pub(super) struct DirectionColors {
    pub downward: &'static str,
    pub upward: &'static str,
    pub cycle: &'static str,
    pub count_bg: &'static str,
}

#[allow(clippy::struct_field_names)] // "_fill" suffix groups related color values
pub(super) struct NodeSelectionColors {
    pub crate_fill: &'static str,
    pub module_fill: &'static str,
    pub external_fill: &'static str,
    pub external_transitive_fill: &'static str,
}

pub(super) struct RelationColors {
    pub dependency: &'static str,
    pub dependent: &'static str,
    pub dimmed: &'static str,
}

#[allow(dead_code)]
pub(super) struct ToolbarColors {
    pub bg: &'static str,
    pub border: &'static str,
    pub btn_fill: &'static str,
    pub btn_hover: &'static str,
    pub btn_stroke: &'static str,
    pub checkbox: &'static str,
    pub checkbox_checked: &'static str,
    pub separator: &'static str,
}

#[allow(dead_code)]
pub(super) struct ColorPalette {
    pub nodes: NodeColors,
    pub direction: DirectionColors,
    pub node_selection: NodeSelectionColors,
    pub relation: RelationColors,
    pub toolbar: ToolbarColors,
}

pub(super) static COLORS: ColorPalette = ColorPalette {
    nodes: NodeColors {
        crate_fill: BLUE_100,
        crate_stroke: BLUE,
        module_fill: ORANGE_100,
        module_stroke: ORANGE,
        external_section_fill: GRAY_200,
        external_section_stroke: GRAY_400,
        external_crate_fill: GRAY_200,
        external_crate_stroke: GRAY_600,
        external_transitive_fill: GRAY_100,
        external_transitive_stroke: "#bbb",
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
        external_fill: GRAY_300,
        external_transitive_fill: GRAY_200,
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
pub(super) struct NodeClasses {
    pub crate_node: &'static str,
    pub module: &'static str,
    pub external_section: &'static str,
    pub external_crate: &'static str,
    pub external_transitive: &'static str,
    pub label: &'static str,
    pub child_count: &'static str,
    pub tree_line: &'static str,
    pub collapse_toggle: &'static str,
    pub collapsed: &'static str,
}

#[allow(dead_code)]
pub(super) struct DirectionClasses {
    pub dep_arc: &'static str,
    pub downward: &'static str,
    pub upward: &'static str,
    pub dep_arrow: &'static str,
    pub upward_arrow: &'static str,
    pub cycle_arc: &'static str,
    pub cycle_arrow: &'static str,
    pub arc_hitarea: &'static str,
    pub crate_dep_arc: &'static str,
    pub module_dep_arc: &'static str,
    pub virtual_arc: &'static str,
    pub virtual_arrow: &'static str,
    pub virtual_hitarea: &'static str,
}

#[allow(dead_code)]
pub(super) struct NodeSelectionClasses {
    pub selected_crate: &'static str,
    pub selected_module: &'static str,
    pub selected_external: &'static str,
    pub selected_external_transitive: &'static str,
    pub group_member: &'static str,
    pub cycle_member: &'static str,
}

#[allow(dead_code)]
pub(super) struct RelationClasses {
    pub highlighted_arc: &'static str,
    pub highlighted_arrow: &'static str,
    pub highlighted_label: &'static str,
    pub dep_node: &'static str,
    pub dependent_node: &'static str,
    pub dimmed: &'static str,
    pub has_highlight: &'static str,
    pub shadow_path: &'static str,
    pub glow_incoming: &'static str,
    pub glow_outgoing: &'static str,
    pub glow_cycle: &'static str,
    pub has_pinned: &'static str,
}

#[allow(dead_code)]
pub(super) struct ToolbarClasses {
    pub view_options: &'static str,
    pub btn: &'static str,
    pub btn_text: &'static str,
    pub separator: &'static str,
    pub checkbox: &'static str,
    pub checked: &'static str,
    pub disabled: &'static str,
    pub label: &'static str,
    pub root: &'static str,
    pub html_btn: &'static str,
    pub separator_v: &'static str,
    pub toggle: &'static str,
    pub search_group: &'static str,
    pub search_input_wrapper: &'static str,
    pub search_clear: &'static str,
    pub scope: &'static str,
    pub scope_btn: &'static str,
    pub scope_active: &'static str,
    pub result_count: &'static str,
    pub dropdown: &'static str,
    pub dropdown_btn: &'static str,
    pub dropdown_panel: &'static str,
}

#[allow(dead_code, clippy::struct_field_names)] // "search_" prefix groups related CSS classes
pub(super) struct SearchClasses {
    pub search_match: &'static str,
    pub search_match_parent: &'static str,
    pub search_dimmed: &'static str,
}

#[allow(dead_code)]
pub(super) struct LabelClasses {
    pub arc_count: &'static str,
    pub arc_count_bg: &'static str,
    pub arc_count_group: &'static str,
    pub hidden_by_filter: &'static str,
}

#[allow(dead_code)]
pub(super) struct SidebarClasses {
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
    pub arrow: &'static str,
    pub node_crate: &'static str,
    pub node_module: &'static str,
    pub node_from: &'static str,
    pub node_to: &'static str,
    pub node_external: &'static str,
    pub node_external_transitive: &'static str,
    pub node_external_section: &'static str,
    pub node_selected: &'static str,
    pub transient: &'static str,
    pub arc_symbols: &'static str,
    pub ext_info: &'static str,
}

#[allow(dead_code)]
pub(super) struct CssClassNames {
    pub nodes: NodeClasses,
    pub direction: DirectionClasses,
    pub node_selection: NodeSelectionClasses,
    pub relation: RelationClasses,
    pub toolbar: ToolbarClasses,
    pub labels: LabelClasses,
    pub sidebar: SidebarClasses,
    pub search: SearchClasses,
}

pub(super) static CSS: CssClassNames = CssClassNames {
    nodes: NodeClasses {
        crate_node: "crate",
        module: "module",
        external_section: "external-section",
        external_crate: "external",
        external_transitive: "external-transitive",
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
        module_dep_arc: "module-dep-arc",
        virtual_arc: "virtual-arc",
        virtual_arrow: "virtual-arrow",
        virtual_hitarea: "virtual-hitarea",
    },
    node_selection: NodeSelectionClasses {
        selected_crate: "selected-crate",
        selected_module: "selected-module",
        selected_external: "selected-external",
        selected_external_transitive: "selected-external-transitive",
        group_member: "group-member",
        cycle_member: "cycle-member",
    },
    relation: RelationClasses {
        highlighted_arc: "highlighted-arc",
        highlighted_arrow: "highlighted-arrow",
        highlighted_label: "highlighted-label",
        dep_node: "dep-node",
        dependent_node: "dependent-node",
        dimmed: "dimmed",
        has_highlight: "has-highlight",
        shadow_path: "shadow-path",
        glow_incoming: "glow-incoming",
        glow_outgoing: "glow-outgoing",
        glow_cycle: "glow-cycle",
        has_pinned: "has-pinned",
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
        root: "toolbar-root",
        html_btn: "toolbar-html-btn",
        separator_v: "toolbar-separator-v",
        toggle: "toolbar-toggle",
        search_group: "toolbar-search-group",
        search_input_wrapper: "toolbar-search-input-wrapper",
        search_clear: "toolbar-search-clear",
        scope: "toolbar-scope",
        scope_btn: "toolbar-scope-btn",
        scope_active: "active",
        result_count: "toolbar-result-count",
        dropdown: "toolbar-dropdown",
        dropdown_btn: "toolbar-dropdown-btn",
        dropdown_panel: "toolbar-dropdown-panel",
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
        arrow: "sidebar-arrow",
        node_crate: "sidebar-node-crate",
        node_module: "sidebar-node-module",
        node_from: "sidebar-node-from",
        node_to: "sidebar-node-to",
        node_external: "sidebar-node-external",
        node_external_transitive: "sidebar-node-external-transitive",
        node_external_section: "sidebar-node-external-section",
        node_selected: "sidebar-node-selected",
        transient: "sidebar-transient",
        arc_symbols: "sidebar-arc-symbols",
        ext_info: "sidebar-ext-info",
    },
    search: SearchClasses {
        search_match: "search-match",
        search_match_parent: "search-match-parent",
        search_dimmed: "search-dimmed",
    },
};

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
    fn test_group_member_class_exists() {
        assert_eq!(CSS.node_selection.group_member, "group-member");
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
        assert!(!CSS.direction.module_dep_arc.is_empty());
        assert!(!CSS.direction.virtual_arc.is_empty());
        assert!(!CSS.direction.virtual_arrow.is_empty());
        assert!(!CSS.direction.virtual_hitarea.is_empty());

        assert!(!CSS.node_selection.selected_crate.is_empty());
        assert!(!CSS.node_selection.selected_module.is_empty());
        assert!(!CSS.node_selection.selected_external.is_empty());
        assert!(!CSS.node_selection.group_member.is_empty());

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
        assert!(!CSS.toolbar.root.is_empty());
        assert!(!CSS.toolbar.html_btn.is_empty());
        assert!(!CSS.toolbar.separator_v.is_empty());
        assert!(!CSS.toolbar.toggle.is_empty());
        assert!(!CSS.toolbar.search_group.is_empty());
        assert!(!CSS.toolbar.search_input_wrapper.is_empty());
        assert!(!CSS.toolbar.search_clear.is_empty());
        assert!(!CSS.toolbar.scope.is_empty());
        assert!(!CSS.toolbar.scope_btn.is_empty());
        assert!(!CSS.toolbar.scope_active.is_empty());
        assert!(!CSS.toolbar.result_count.is_empty());

        assert!(!CSS.search.search_match.is_empty());
        assert!(!CSS.search.search_match_parent.is_empty());
        assert!(!CSS.search.search_dimmed.is_empty());

        assert!(!CSS.labels.arc_count.is_empty());
        assert!(!CSS.labels.arc_count_bg.is_empty());
        assert!(!CSS.labels.arc_count_group.is_empty());
        assert!(!CSS.labels.hidden_by_filter.is_empty());
    }
}
