# ADR-015: Use foreignObject HTML for SVG Panels

- **Status:** Active
- **Decided:** 2026-01-31

## Context

Interactive UI elements (sidebar with collapse, scroll, text selection) need HTML features that SVG-native elements do not provide. Standalone SVG has no surrounding HTML document — an HTML overlay is not possible.

## Decision

We render UI panels as HTML inside SVG `foreignObject` elements.

## Rationale

SVG renders the graph, but code references are text — and text is easier to style with the CSS box model in HTML than in native SVG. Since cargo-arc produces a self-contained SVG file, we embed HTML via foreignObject. JavaScript manipulates the DOM for interactivity.

## Consequences

### Positive
- Flexbox, scrolling, text selection in panels
- Consistent with self-contained SVG (no external HTML wrapper)

### Negative
- foreignObject support varies between SVG viewers (browsers OK, some tools not)
- CSS isolation between SVG and HTML requires care
- Toolbar and sidebar cannot stay fixed on screen: they disappear briefly during fast scrolling. Making the handler that repositions them cheaper does not help, and neither does CSS inside the SVG. Only an HTML page that holds the panels outside the SVG avoids it.
- Repositioning the panels on scroll can make Chromium keep scrolling on its own. Scroll anchoring is therefore turned off for the diagram, and must stay off as long as panels move with scrolling.
