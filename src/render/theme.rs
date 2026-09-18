//! Named themes, each light or dark, and the palette a theme paints the
//! diagram with. A palette field is one CSS custom property: the rules in
//! `css.rs` read `var(--arc-…)` and every theme declares the values.

/// Whether a theme is meant for a light or a dark surrounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    /// The word the page, the service line and the `data-mode` attribute use.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// The mode a word names, if any.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// A named palette with the mode it is made for. `name` is what `--theme`
/// and `data-theme` take, `label` what the page shows.
#[derive(Debug)]
pub struct Theme {
    pub name: &'static str,
    pub label: &'static str,
    pub mode: Mode,
    pub(super) palette: ColorPalette,
}

/// Every shipped theme; the first of each mode is that mode's default.
pub static THEMES: [&Theme; 2] = [&LATTE, &MOCHA];

impl Theme {
    /// The theme called `name`; `light` and `dark` name the default theme
    /// of that mode.
    #[must_use]
    pub fn named(name: &str) -> Option<&'static Theme> {
        match Mode::parse(name) {
            Some(mode) => Some(Self::default_for(mode)),
            None => THEMES.iter().copied().find(|theme| theme.name == name),
        }
    }

    /// The first theme of `mode`.
    ///
    /// # Panics
    /// If [`THEMES`] ships no theme of `mode`.
    #[must_use]
    pub fn default_for(mode: Mode) -> &'static Theme {
        Self::of_mode(mode).next().expect("both modes ship a theme")
    }

    /// The themes of `mode`, the default first.
    pub fn of_mode(mode: Mode) -> impl Iterator<Item = &'static Theme> {
        THEMES
            .iter()
            .copied()
            .filter(move |theme| theme.mode == mode)
    }
}

/// A group of palette colours. Each field is the CSS custom property
/// `--arc-<prefix>-<name>`: `VARS` holds a `var(…)` reference per field for
/// the rules, `variables` the property and value pairs a theme declares.
macro_rules! color_group {
    (
        $(#[$meta:meta])*
        $group:ident, $prefix:literal {
            $($(#[$field_meta:meta])* $field:ident => $name:literal),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy)]
        pub(super) struct $group {
            $($(#[$field_meta])* pub $field: &'static str,)*
        }

        impl $group {
            pub(super) const VARS: Self = Self {
                $($field: concat!("var(--arc-", $prefix, "-", $name, ")"),)*
            };

            pub(super) fn variables(&self) -> impl Iterator<Item = (&'static str, &'static str)> {
                [$((concat!("--arc-", $prefix, "-", $name), self.$field),)*].into_iter()
            }
        }
    };
}

color_group!(NodeColors, "node" {
    crate_fill => "crate-fill",
    crate_stroke => "crate-stroke",
    module_fill => "module-fill",
    module_stroke => "module-stroke",
    external_section_fill => "external-section-fill",
    external_section_stroke => "external-section-stroke",
    external_crate_fill => "external-crate-fill",
    external_crate_stroke => "external-crate-stroke",
    external_transitive_fill => "external-transitive-fill",
    external_transitive_stroke => "external-transitive-stroke",
    /// Label text inside a node.
    text => "text",
    tree_line => "tree-line",
    child_count => "child-count",
    collapse_toggle => "collapse-toggle",
    collapse_hover => "collapse-hover",
});

color_group!(DirectionColors, "direction" {
    downward => "downward",
    upward => "upward",
    cycle => "cycle",
    count_bg => "count-bg",
    reexport => "reexport",
});

color_group!(
    #[allow(clippy::struct_field_names)] // "_fill" suffix groups related color values
    NodeSelectionColors, "selection" {
        crate_fill => "crate-fill",
        module_fill => "module-fill",
        external_fill => "external-fill",
        external_transitive_fill => "external-transitive-fill",
    }
);

color_group!(RelationColors, "relation" {
    dependency => "dependency",
    dependent => "dependent",
    dimmed => "dimmed",
});

color_group!(
    /// The backing glow behind a highlighted arc. Separate from the relation
    /// colours so a glow can differ from the arc it backs.
    GlowColors, "glow" {
        incoming => "incoming",
        outgoing => "outgoing",
        cycle => "cycle",
    }
);

color_group!(
    /// The HTML toolbar in its foreignObject.
    ToolbarColors, "toolbar" {
        bg => "bg",
        border => "border",
        text => "text",
        /// Buttons, inputs, the dropdown panel.
        control_bg => "control-bg",
        control_border => "control-border",
        btn_hover => "btn-hover",
        row_hover => "row-hover",
        checkbox_border => "checkbox-border",
        /// Focus ring, the active scope button, the search-match-parent outline.
        accent => "accent",
        on_accent => "on-accent",
        text_muted => "text-muted",
        text_faint => "text-faint",
        pressed_bg => "pressed-bg",
        pressed_border => "pressed-border",
        pressed_text => "pressed-text",
        panel_shadow => "panel-shadow",
    }
);

color_group!(
    /// The HTML sidebar. Node badges take their colours from [`NodeColors`]
    /// and [`NodeSelectionColors`]; only what has no diagram counterpart is here.
    SidebarColors, "sidebar" {
        bg => "bg",
        border => "border",
        text => "text",
        text_muted => "text-muted",
        tag_border => "tag-border",
        row_focus_bg => "row-focus-bg",
        ordinal_bg => "ordinal-bg",
        badge_bg => "badge-bg",
        badge_text => "badge-text",
        /// The one selected badge whose border is not the node's stroke: the
        /// transitive stroke is too faint for a border on the sidebar surface.
        node_external_transitive_selected_border => "external-transitive-selected-border",
    }
);

color_group!(
    /// The jump popover next to a hovered node (`js/jump_icons.js`).
    PopoverColors, "popover" {
        bg => "bg",
        border => "border",
        chip_bg => "chip-bg",
        chip_border => "chip-border",
        chip_hover_bg => "chip-hover-bg",
        chip_hover_border => "chip-hover-border",
        chip_text => "chip-text",
    }
);

color_group!(
    /// The body of the HTML page around the diagram.
    PageColors, "page" {
        bg => "bg",
    }
);

#[derive(Debug, Clone, Copy)]
pub(super) struct ColorPalette {
    pub nodes: NodeColors,
    pub direction: DirectionColors,
    pub node_selection: NodeSelectionColors,
    pub relation: RelationColors,
    pub glow: GlowColors,
    pub toolbar: ToolbarColors,
    pub sidebar: SidebarColors,
    pub popover: PopoverColors,
    pub page: PageColors,
}

impl ColorPalette {
    /// A palette whose every colour is the `var(…)` reference to itself.
    pub(super) const VARS: Self = Self {
        nodes: NodeColors::VARS,
        direction: DirectionColors::VARS,
        node_selection: NodeSelectionColors::VARS,
        relation: RelationColors::VARS,
        glow: GlowColors::VARS,
        toolbar: ToolbarColors::VARS,
        sidebar: SidebarColors::VARS,
        popover: PopoverColors::VARS,
        page: PageColors::VARS,
    };

    /// Every custom property with this palette's value.
    pub(super) fn variables(&self) -> impl Iterator<Item = (&'static str, &'static str)> {
        self.nodes
            .variables()
            .chain(self.direction.variables())
            .chain(self.node_selection.variables())
            .chain(self.relation.variables())
            .chain(self.glow.variables())
            .chain(self.toolbar.variables())
            .chain(self.sidebar.variables())
            .chain(self.popover.variables())
            .chain(self.page.variables())
    }
}

/// Catppuccin Latte accents on Tailwind tints and neutral greys: the
/// palette the diagram had before it could be themed.
mod latte {
    pub const GREEN: &str = "#40a02b";
    pub const YELLOW: &str = "#df8e1d";
    /// Cluster / cycle mass. A muted rose (Catppuccin Latte Flamingo), not a
    /// vivid red: it paints the whole strongly-connected cluster, so it must
    /// recede into context.
    pub const RED: &str = "#dd7878";
    pub const PURPLE: &str = "#8839ef";
    pub const BLUE: &str = "#1e66f5";
    pub const ORANGE: &str = "#fe640b";
    pub const TEAL: &str = "#179299";

    pub const BLUE_100: &str = "#dbeafe";
    pub const BLUE_300: &str = "#93c5fd";
    pub const ORANGE_100: &str = "#ffedd5";
    pub const ORANGE_300: &str = "#fdba74";

    pub const GRAY_600: &str = "#666";
    pub const GRAY_400: &str = "#888";
    pub const GRAY_300: &str = "#ccc";
    pub const GRAY_200: &str = "#e0e0e0";
    pub const GRAY_100: &str = "#f5f5f5";
    pub const GRAY_50: &str = "#fafafa";
    pub const WHITE: &str = "#fff";
}

pub static LATTE: Theme = {
    use latte::{
        BLUE, BLUE_100, BLUE_300, GRAY_50, GRAY_100, GRAY_200, GRAY_300, GRAY_400, GRAY_600, GREEN,
        ORANGE, ORANGE_100, ORANGE_300, PURPLE, RED, TEAL, WHITE, YELLOW,
    };
    Theme {
        name: "latte",
        label: "Catppuccin Latte",
        mode: Mode::Light,
        palette: ColorPalette {
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
                text: "#000",
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
                reexport: TEAL,
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
            glow: GlowColors {
                incoming: GREEN,
                outgoing: PURPLE,
                cycle: RED,
            },
            toolbar: ToolbarColors {
                bg: "#f8f8f8",
                border: GRAY_200,
                text: "#333",
                control_bg: WHITE,
                control_border: GRAY_300,
                btn_hover: "#e8e8e8",
                row_hover: "#f0f0f0",
                checkbox_border: "#999",
                accent: "#4a90d9",
                on_accent: WHITE,
                text_muted: GRAY_400,
                text_faint: "#999",
                pressed_bg: BLUE_100,
                pressed_border: "#60a5fa",
                pressed_text: "#1e3a8a",
                panel_shadow: "0 2px 8px rgba(0,0,0,0.12)",
            },
            sidebar: SidebarColors {
                bg: GRAY_50,
                border: GRAY_200,
                text: GRAY_600,
                text_muted: GRAY_400,
                tag_border: GRAY_300,
                row_focus_bg: GRAY_100,
                ordinal_bg: GRAY_200,
                badge_bg: BLUE_100,
                badge_text: BLUE,
                node_external_transitive_selected_border: GRAY_400,
            },
            popover: PopoverColors {
                bg: WHITE,
                border: GRAY_300,
                chip_bg: GRAY_100,
                chip_border: GRAY_300,
                chip_hover_bg: BLUE_100,
                chip_hover_border: BLUE_300,
                chip_text: GRAY_600,
            },
            page: PageColors { bg: WHITE },
        },
    }
};

/// Catppuccin Mocha. The tinted fills are the base blended with the
/// accent (a fifth for a node, two fifths for a selected node), the way
/// the Tailwind tints sit on white in Latte.
mod mocha {
    pub const BASE: &str = "#1e1e2e";
    pub const MANTLE: &str = "#181825";
    pub const SURFACE0: &str = "#313244";
    pub const SURFACE1: &str = "#45475a";
    pub const SURFACE2: &str = "#585b70";
    pub const OVERLAY0: &str = "#6c7086";
    pub const OVERLAY1: &str = "#7f849c";
    pub const OVERLAY2: &str = "#9399b2";
    pub const SUBTEXT0: &str = "#a6adc8";
    pub const SUBTEXT1: &str = "#bac2de";
    pub const TEXT: &str = "#cdd6f4";
    pub const GREEN: &str = "#a6e3a1";
    pub const YELLOW: &str = "#f9e2af";
    pub const FLAMINGO: &str = "#f2cdcd";
    pub const MAUVE: &str = "#cba6f7";
    pub const BLUE: &str = "#89b4fa";
    pub const PEACH: &str = "#fab387";
    pub const TEAL: &str = "#94e2d5";
    pub const LAVENDER: &str = "#b4befe";
    /// Base blended with blue.
    pub const BLUE_TINT: &str = "#333c57";
    pub const BLUE_TINT_STRONG: &str = "#495a80";
    /// Base blended with peach.
    pub const PEACH_TINT: &str = "#4a3c40";
    pub const PEACH_TINT_STRONG: &str = "#765a52";
    /// Between base and surface0, for the transitive externals.
    pub const BASE_RAISED: &str = "#27273a";
}

pub static MOCHA: Theme = {
    use mocha::{
        BASE, BASE_RAISED, BLUE, BLUE_TINT, BLUE_TINT_STRONG, FLAMINGO, GREEN, LAVENDER, MANTLE,
        MAUVE, OVERLAY0, OVERLAY1, OVERLAY2, PEACH, PEACH_TINT, PEACH_TINT_STRONG, SUBTEXT0,
        SUBTEXT1, SURFACE0, SURFACE1, SURFACE2, TEAL, TEXT, YELLOW,
    };
    Theme {
        name: "mocha",
        label: "Catppuccin Mocha",
        mode: Mode::Dark,
        palette: ColorPalette {
            nodes: NodeColors {
                crate_fill: BLUE_TINT,
                crate_stroke: BLUE,
                module_fill: PEACH_TINT,
                module_stroke: PEACH,
                external_section_fill: SURFACE0,
                external_section_stroke: OVERLAY2,
                external_crate_fill: SURFACE0,
                external_crate_stroke: SUBTEXT0,
                external_transitive_fill: BASE_RAISED,
                external_transitive_stroke: OVERLAY0,
                text: TEXT,
                tree_line: OVERLAY2,
                child_count: OVERLAY1,
                collapse_toggle: SUBTEXT0,
                collapse_hover: BLUE,
            },
            direction: DirectionColors {
                downward: GREEN,
                upward: YELLOW,
                cycle: FLAMINGO,
                count_bg: BASE,
                reexport: TEAL,
            },
            node_selection: NodeSelectionColors {
                crate_fill: BLUE_TINT_STRONG,
                module_fill: PEACH_TINT_STRONG,
                external_fill: SURFACE2,
                external_transitive_fill: SURFACE1,
            },
            relation: RelationColors {
                dependency: GREEN,
                dependent: MAUVE,
                dimmed: OVERLAY0,
            },
            glow: GlowColors {
                incoming: GREEN,
                outgoing: MAUVE,
                cycle: FLAMINGO,
            },
            toolbar: ToolbarColors {
                bg: MANTLE,
                border: SURFACE0,
                text: TEXT,
                control_bg: BASE,
                control_border: SURFACE1,
                btn_hover: SURFACE0,
                row_hover: SURFACE0,
                checkbox_border: OVERLAY0,
                accent: BLUE,
                on_accent: BASE,
                text_muted: OVERLAY1,
                text_faint: OVERLAY0,
                pressed_bg: BLUE_TINT,
                pressed_border: BLUE,
                pressed_text: LAVENDER,
                panel_shadow: "0 2px 8px rgba(0,0,0,0.5)",
            },
            sidebar: SidebarColors {
                bg: MANTLE,
                border: SURFACE0,
                text: SUBTEXT1,
                text_muted: OVERLAY1,
                tag_border: SURFACE1,
                row_focus_bg: SURFACE0,
                ordinal_bg: SURFACE1,
                badge_bg: BLUE_TINT,
                badge_text: BLUE,
                node_external_transitive_selected_border: OVERLAY2,
            },
            popover: PopoverColors {
                bg: BASE,
                border: SURFACE1,
                chip_bg: SURFACE0,
                chip_border: SURFACE1,
                chip_hover_bg: BLUE_TINT,
                chip_hover_border: BLUE_TINT_STRONG,
                chip_text: SUBTEXT1,
            },
            page: PageColors { bg: BASE },
        },
    }
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latte_keeps_the_original_palette() {
        assert_eq!(LATTE.palette.nodes.crate_fill, "#dbeafe");
        assert_eq!(LATTE.palette.direction.downward, "#40a02b");
        assert_eq!(LATTE.palette.sidebar.bg, "#fafafa");
        assert_eq!(LATTE.palette.node_selection.crate_fill, "#93c5fd");
        assert_eq!(LATTE.palette.relation.dependent, "#8839ef");
    }

    #[test]
    fn named_resolves_names_and_mode_aliases() {
        assert_eq!(Theme::named("latte").map(|t| t.name), Some("latte"));
        assert_eq!(Theme::named("mocha").map(|t| t.name), Some("mocha"));
        assert_eq!(Theme::named("light").map(|t| t.name), Some("latte"));
        assert_eq!(Theme::named("dark").map(|t| t.name), Some("mocha"));
        assert!(Theme::named("frappe").is_none());
    }

    #[test]
    fn a_field_is_one_custom_property() {
        assert_eq!(NodeColors::VARS.crate_fill, "var(--arc-node-crate-fill)");
        let (name, value) = LATTE.palette.nodes.variables().next().unwrap();
        assert_eq!((name, value), ("--arc-node-crate-fill", "#dbeafe"));
    }
}
