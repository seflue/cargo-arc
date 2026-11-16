# ADR-001: Use Deterministic Tree Layout with Dependency Arcs

- **Status:** Active
- **Decided:** 2026-01-20

## Context

Existing tools (cargo-depgraph, cargo-modules) produce DOT format for GraphViz. The resulting layouts offer no control over the concrete positioning of individual nodes and no ability to add interactive features (hover, collapse, selection).

## Decision

We represent crates and modules as an indented tree structure and draw dependencies as quadratic Bezier arcs beside it, upward edges on its left and every other dependency on its right. We compute the layout ourselves — identical input produces identical positions.

## Rationale

- Control over positioning enables the Tree+Arc layout
- Interactive features (hover, collapse, selection) require custom rendering
- Deterministic: SVG output is reproducible and diffable
- Evaluation (2026-02-05): Indented-Tree+Arc hybrid is a niche that no existing library covers

## Consequences

### Positive
- Interactivity directly in SVG without a framework
- Own visual style, not tied to GraphViz aesthetics

### Negative
- Custom layout computation must be maintained
- No fallback to standard graph layouts

## References

- Wattenberg, M. (2002). "Arc Diagrams: Visualizing Structure in Strings." IEEE INFOVIS 2002, pp. 110-116. [IEEE Xplore](https://ieeexplore.ieee.org/document/1173155/)
- Prior Art: Saaty (1964) — Arc representations as a general visualization concept
