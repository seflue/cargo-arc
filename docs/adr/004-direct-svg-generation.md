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

## Consequences

### Positive
- Control over SVG structure and interactivity

### Negative
- No access to layout algorithms of existing libraries
- SVG generation must be maintained manually
