# ADR-006: Embed All Metadata in Self-Contained SVG

- **Status:** Active
- **Decided:** 2026-01-20

## Context

cargo-arc produces SVGs that are opened directly in the browser (file:// URL). There is no surrounding HTML document and no web server. All data, styles, and interaction logic must live inside the SVG itself.

## Decision

We embed CSS, JavaScript, and structured metadata (`STATIC_DATA` as a JS constant) directly into the SVG file. No external files.

Rust is the single source of truth for all metadata. The DOM is never read as a data source — `STATIC_DATA` contains everything JS needs at runtime (see ADR-011).

## Rationale

- Standalone SVG works with file:// URLs without a web server
- One file = simple distribution and archiving
- DOM reading eliminated (prevents divergence between data and presentation)

## Consequences

### Positive
- SVG is portable — one file, no build tool or web server needed

### Negative
- SVG file size grows with embedded data and JS
