# ADR-006: Embed All Metadata in Self-Contained SVG

- **Status:** Active
- **Decided:** 2026-01-20
- Corrected 2026-10-13: the output is not only an SVG opened over file://. The same content is also written as an HTML page with the SVG inline, and `cargo arc ui` serves that page (ADR-027). The served page loads no code, styles or data from the service; it only sends jump and command requests to it and receives its events.
- Corrected 2026-10-13: the rule that the DOM is never read as a data source was wider than meant. ADR-011 states it narrower: interaction state and derived data are never read from the DOM; rendering code may read back geometry that it wrote itself.

## Context

cargo-arc writes the diagram as a bare SVG or as an HTML page with the SVG inline. Either file is opened directly in the browser (file:// URL), without a web server. All data, styles, and interaction logic must live inside the output itself.

## Decision

We embed CSS, JavaScript, and structured metadata (`STATIC_DATA` as a JS constant) directly into the output, the SVG or the page around it. No external files.

Rust is the single source of truth for all metadata. `STATIC_DATA` contains everything JS needs at runtime; interaction state and derived data are never read from the DOM (see ADR-011).

## Rationale

- Standalone SVG works with file:// URLs without a web server
- One file = simple distribution and archiving
- DOM reading eliminated (prevents divergence between data and presentation)

## Consequences

### Positive
- SVG is portable — one file, no build tool or web server needed

### Negative
- SVG file size grows with embedded data and JS
