//! The toolbar frame shared by both pages: the `foreignObject`/`toolbar-root`
//! wrapper, the View dropdown with the appearance block inside it, Follow
//! editor and the cross-link to the workspace's other page. `elements::render_toolbar`
//! (the arc diagram) and `hotspots::render_toolbar` (the hotspot map) each
//! weave their own buttons into [`Content`] and call [`render`]; the arc
//! page fills every field, the map leaves them at their `Default`.

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

/// The toolbar frame: the `foreignObject`/`toolbar-root` wrapper, the View
/// dropdown (`content.dropdown_filters` above a divider, then the
/// appearance block every page gets), Follow editor (under
/// `config.with_jump_ids`, like the rest of a served page's service
/// toggles), the cross-link and the jump-status span, with `content`'s
/// page-specific buttons woven in around them.
#[allow(
    clippy::cast_possible_truncation,
    reason = "SVG pixel coordinates fit in i32"
)]
pub(super) fn render(
    width: f32,
    config: &RenderConfig,
    content: &Content,
    cross_link: CrossLink,
) -> String {
    let ct = &CSS.toolbar;
    let height = LAYOUT.toolbar.height as i32;

    let divider = if content.dropdown_filters.is_empty() {
        String::new()
    } else {
        format!("          <div class=\"{}\"></div>\n", ct.dropdown_divider)
    };

    // Only a page served by `cargo arc ui` has an editor to follow; a file
    // written by `cargo arc -o` has none.
    let follow = if config.with_jump_ids {
        format!(
            "      <button id=\"follow-toggle\" class=\"{} {}\" aria-pressed=\"true\">Follow editor</button>\n",
            ct.html_btn, ct.follow_toggle,
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
            "  <foreignObject id=\"toolbar-fo\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"",
            " style=\"display:none; overflow:visible\">\n",
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
            "    </div>\n",
            "  </foreignObject>\n",
        ),
        width,                    // foreignObject width
        height,                   // foreignObject height
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
        follow,                   // Follow editor, under with_jump_ids
        content.after_follow,     // page-specific service toggles after Follow editor
        page_link,                // link to the workspace's other page
        ct.jump_status,           // .toolbar-jump-status
    )
}
