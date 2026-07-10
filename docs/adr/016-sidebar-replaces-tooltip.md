# ADR-016: Replace Tooltip with Persistent Sidebar

- **Status:** Active
- **Decided:** 2026-01-31

## Context

Dependency details were shown as a tooltip. Tooltips disappear when the mouse moves away — not ideal for content that needs to be read and understood.

## Decision

We show dependency details in a foreignObject-based sidebar.

## Rationale

The sidebar can be pinned via click so that content remains visible. With the sidebar also used for hover, we have a unified model for detail display — the tooltip became redundant.

## Consequences

### Positive
- Persistent detail display via pin

### Negative
- Sidebar needs more space than a tooltip
