# ADR-025: Place Toolbar and Sidebar beside the SVG on a Page

- **Status:** Active
- **Decided:** 2026-09-29
- **Supersedes:** ADR-015 (for the page)
- **Related:** ADR-006 (self-contained output), ADR-016 (sidebar), ADR-027 (the `ui` service)

## Context

cargo-arc writes the diagram in two forms.
A bare SVG goes to stdout or to an `.svg` file.
A page is an HTML document with the SVG inline; `-o` writes it for an `.html` or `.xhtml` file, and `cargo arc ui` serves it to a browser tab or an editor plugin (ADR-027).

ADR-015 put the toolbar and the sidebar into the SVG as HTML inside `foreignObject` elements, because a bare SVG has no HTML document around it.
A page has one.

Inside the SVG the panels cannot stay fixed on the screen.
A script moves them after every scroll event.
Browsers scroll before that event reaches the script, so during fast scrolling the panels leave the screen until the script moves them back.
ADR-015 lists this among its drawbacks.

## Decision

On a page, the toolbar and the sidebar are HTML elements outside the SVG, fixed to the window.
No script moves them when the diagram scrolls.
The sidebar sits at the right edge of the window.

The bare SVG keeps both panels in `foreignObject` elements, as ADR-015 decided.
The hotspot map keeps its sidebar in a `foreignObject` in both forms.

## Rationale

- A fixed element in an HTML document stays on the screen while the content under it scrolls. The browser does that without a script, so the panels stay visible during fast scrolling.
- A bare SVG has no HTML document, and `foreignObject` is still the only way to put HTML panels into it.
- A sidebar at the window edge does not depend on where the arcs end. Nothing measures the arcs to place it, and a tall sidebar does not make the canvas grow.

## Consequences

- Each panel has two hosts. Both wrap the same inner markup, and tests check that the two hosts carry the same root block.
- The `foreignObject` form is now used only by the bare SVG, and a browser shows it less often than the page.
- On a page the panels come after the SVG and before the script. Placed before the SVG, a lookup of the first `svg` element would find a sidebar icon. Placed after the script, the panels would not exist when it runs.
- The script has to parse as HTML and as XML, because a browser reads an `.html` file with its HTML parser.
- On a wide window the sidebar sits at the window edge, away from the arcs it describes.
- A click in the toolbar on a page does not reach the SVG. It does not clear the selection and does not close the view menu.
- On a page the open view menu lies above the sidebar.
- Scroll anchoring stays off for the bare SVG, where the panels still move with scrolling.

## References

- `src/render/constants.rs`: `Document`, the choice between the bare SVG and the page.
- `src/render/mod.rs`: the order of SVG, panels and script.
- `src/render/elements.rs`, `src/render/toolbar.rs`: both hosts of each panel.
