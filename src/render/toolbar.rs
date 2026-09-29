//! The toolbar frame shared by both pages: the `toolbar-root` markup, the View
//! dropdown with the appearance block inside it, Follow editor, Recompute on
//! save and the cross-link to the workspace's other page. `elements::render_toolbar`
//! (the arc diagram) and `hotspots::render_toolbar` (the hotspot map) each
//! weave their own buttons into [`Content`] and call [`render`]; the arc
//! page fills every field, the map leaves them at their `Default`. The
//! markup sits in a `foreignObject` inside the SVG ([`in_foreign_object`]) or
//! in a fixed host `div` after it ([`in_page`]).

use super::constants::{CSS, LAYOUT, RenderConfig};

/// A page's own buttons and panels around the shared frame. A page with
/// none of a given kind - the hotspot map has no dropdown filters, search
/// group or extra service toggles - leaves that field at its `Default`.
#[derive(Default)]
pub(super) struct Content {
    /// Rendered before the View dropdown (the arc page's Collapse All
    /// button and its separator).
    pub before_dropdown: String,
    /// Rendered inside the View dropdown panel, above the Appearance block.
    /// Empty on a page with no filters: the divider that would separate
    /// them from Appearance is then left out too.
    pub dropdown_filters: String,
    /// Rendered after the View dropdown (the arc page's search group).
    pub after_dropdown: String,
    /// Rendered after Follow editor (the arc page's externals/tests switches).
    pub after_follow: String,
    /// A full-width line below the buttons (the map's breadcrumb); the bar
    /// grows by [`SECOND_LINE_HEIGHT`] when it is set.
    pub second_line: String,
}

/// One button-high line plus the bar's 8px flex gap above it.
const SECOND_LINE_HEIGHT: f32 = 32.0;

/// Return the bar's height with `content`'s lines.
pub(super) fn height(content: &Content) -> f32 {
    if content.second_line.is_empty() {
        LAYOUT.toolbar.height
    } else {
        LAYOUT.toolbar.height + SECOND_LINE_HEIGHT
    }
}

/// The bar's link to the workspace's other page: JS updates `href` to carry
/// the current selection across (`?select=<file>`), and the link is dead,
/// like a jump link, on a written file.
#[derive(Clone, Copy)]
pub(super) struct CrossLink {
    pub id: &'static str,
    pub href: &'static str,
    pub label: &'static str,
}

/// The `toolbar-root` div: the View dropdown (`content.dropdown_filters`
/// above a divider, then the appearance block every page gets), Follow
/// editor and Recompute on save (under `config.with_jump_ids`, like the rest
/// of a served page's service toggles), the cross-link and the jump-status
/// span, with `content`'s page-specific buttons woven in around them.
pub(super) fn markup(config: &RenderConfig, content: &Content, cross_link: CrossLink) -> String {
    let ct = &CSS.toolbar;

    let divider = if content.dropdown_filters.is_empty() {
        String::new()
    } else {
        format!("          <div class=\"{}\"></div>\n", ct.dropdown_divider)
    };

    // Only a page served by `cargo arc ui` has an editor to follow and a
    // service to recompute; a file written by `cargo arc -o` has neither.
    // The on-save button starts pressed, the service's default; the service
    // sends its actual state when the page connects.
    let follow = if config.with_jump_ids {
        format!(
            concat!(
                "      <button id=\"follow-toggle\" class=\"{} {}\" aria-pressed=\"true\">Follow editor</button>\n",
                "      <button id=\"on-save-toggle\" class=\"{} {}\" aria-pressed=\"true\">Recompute on save</button>\n",
            ),
            ct.html_btn, ct.follow_toggle, ct.html_btn, ct.switch_toggle,
        )
    } else {
        String::new()
    };

    let page_link = format!(
        "      <a id=\"{}\" class=\"{}\" href=\"{}\">{}</a>\n",
        cross_link.id, ct.html_btn, cross_link.href, cross_link.label,
    );

    format!(
        concat!(
            "    <div class=\"{}\" xmlns=\"http://www.w3.org/1999/xhtml\">\n",
            "{}",
            "      <div class=\"{}\">\n",
            "        <button id=\"view-dropdown-btn\" class=\"{} {}\">View \u{25be}</button>\n",
            "        <div class=\"{}\" style=\"display:none\">\n",
            "{}",
            "{}",
            "          <label class=\"{}\">\n",
            "            <span>Appearance</span>\n",
            "            <select id=\"theme-mode\">",
            "<option value=\"light\">Light</option>",
            "<option value=\"dark\">Dark</option>",
            "<option value=\"system\">System</option>",
            "</select>\n",
            "          </label>\n",
            "          <label class=\"{}\">\n",
            "            <span>Light theme</span>\n",
            "            <select id=\"theme-light\"></select>\n",
            "          </label>\n",
            "          <label class=\"{}\">\n",
            "            <span>Dark theme</span>\n",
            "            <select id=\"theme-dark\"></select>\n",
            "          </label>\n",
            "        </div>\n",
            "      </div>\n",
            "{}",
            "{}",
            "{}",
            "{}",
            "      <span id=\"jump-status\" class=\"{}\"></span>\n",
            "{}",
            "    </div>\n",
        ),
        ct.root,                  // .toolbar-root
        content.before_dropdown,  // page-specific buttons before the dropdown
        ct.dropdown,              // .toolbar-dropdown container
        ct.html_btn,              // dropdown button base class
        ct.dropdown_btn,          // dropdown button marker class
        ct.dropdown_panel,        // .toolbar-dropdown-panel
        content.dropdown_filters, // page-specific filters inside the panel
        divider,                  // divider before Appearance, if there were filters
        ct.select,                // label.toolbar-select (mode switch)
        ct.select,                // label.toolbar-select (light theme)
        ct.select,                // label.toolbar-select (dark theme)
        content.after_dropdown,   // page-specific content after the dropdown
        follow,                   // Follow editor and Recompute on save, under with_jump_ids
        content.after_follow,     // page-specific service toggles after Follow editor
        page_link,                // link to the workspace's other page
        ct.jump_status,           // .toolbar-jump-status
        content.second_line,      // page-specific line below the buttons
    )
}

/// `markup` in the toolbar's `foreignObject`, `width` wide and as high as
/// `content`'s lines.
#[allow(
    clippy::cast_possible_truncation,
    reason = "SVG pixel coordinates fit in i32"
)]
pub(super) fn in_foreign_object(width: f32, content: &Content, markup: &str) -> String {
    format!(
        concat!(
            "  <foreignObject id=\"toolbar-fo\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"",
            " style=\"display:none; overflow:visible\">\n",
            "{}",
            "  </foreignObject>\n",
        ),
        width,
        height(content) as i32,
        markup,
    )
}

/// `markup` in the fixed host `div` that follows the root SVG on a page.
pub(super) fn in_page(markup: &str) -> String {
    format!(
        "<div class=\"{}\">\n{}</div>\n",
        CSS.toolbar.page_host, markup,
    )
}

/// The toolbar frame in the SVG: `markup` in its `foreignObject`.
pub(super) fn render(
    width: f32,
    config: &RenderConfig,
    content: &Content,
    cross_link: CrossLink,
) -> String {
    in_foreign_object(width, content, &markup(config, content, cross_link))
}
