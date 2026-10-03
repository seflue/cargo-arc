# ADR-004: Generate SVG Directly in Rust

- **Status:** Active
- **Decided:** 2026-01-20

## Context

cargo-modules and cargo-depgraph produce DOT format, which is rendered externally by GraphViz. For interactive features, cargo-arc needs control over the SVG. An evaluation (2026-02-05) examined D3.js, Cytoscape, dagre, ELK — all assume standard graph layouts, none supports the Tree+Arc hybrid from ADR-001.

## Decision

We generate SVG programmatically in Rust (`format!`-based). No DOT intermediate format, no external renderer, no graph visualization library.

## Rationale

- Custom layout (Tree+Arcs) requires custom positioning — no library offers this layout
- SVG primitives (rect, text, path) are directly generatable
- Embedded interactive JavaScript is possible (see ADR-006)
- Every node and arc is an element: CSS styles it, events reach it, and the browser finds what lies under the pointer

## Rejected Alternatives

### Drawing into a canvas

A `<canvas>` holds pixels, not elements, so the page would have to take over what SVG provides:

- Hover, highlighting, dimming and filtering rest on elements with CSS classes and `pointer-events`. On a canvas, each needs its own hit test and a redraw of everything after every state change.
- Themes reach the diagram through CSS custom properties, which a canvas does not read.
- The output would no longer be a standalone SVG file (ADR-006).
- Tests assert classes and attributes on DOM elements; a canvas offers only pixels.

A canvas pays off when the number of elements makes DOM layout and styling the cost, or for continuous animation. Neither applies. Chromium's slow rasterizing of many thin antialiased arcs while scrolling, which prompted the question, is fixed in CSS by drawing arcs without antialiasing outside Firefox.

## Consequences

### Positive
- Control over SVG structure and interactivity

### Negative
- No access to layout algorithms of existing libraries
- SVG generation must be maintained manually
