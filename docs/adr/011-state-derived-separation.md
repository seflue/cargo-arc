# ADR-011: Separate State and Derived Data in Frontend

- **Status:** Active
- **Decided:** 2026-01-29
- Corrected 2026-10-13: State holds seven fields, not two. The rule that the DOM is never read as a data source was wider than meant. Interaction state and derived data are never read from the DOM; rendering code may read back geometry that it wrote itself.

## Context

The DOM served simultaneously as state container and view. Code read e.g. `arc.style.strokeWidth` as the "original" — but the value could already have been changed by highlighting. Recurring bugs: highlight growth, timing dependencies between clear/apply.

## Decision

Three-layer model for the frontend architecture:

1. **Static** (immutable, from Rust via `STATIC_DATA`): graph structure, positions, styles
2. **State** (user interaction only): collapsed nodes, click and hover selection, hidden arcs, cluster mode, the selected cluster, expanded sidebar rows
3. **DerivedState** (computed via pure functions from Static+State): visibility, highlights, virtual arcs

Interaction state and derived data are never read from the DOM. Rendering code may read back geometry that it wrote into the DOM itself, such as a node's original width or an arrowhead's points. DerivedState is recomputed on every state change.

## Rationale

- We limit state to what the user did — this reduces the bug surface
- We compute DerivedState as pure functions that we can test deterministically
- Selection needs only one ID — dependencies/dependents are derived from StaticData

## Consequences

### Positive
- No divergence between data and presentation
- DerivedState functions can be tested with pure unit tests
- Highlight growth bugs no longer occur because stroke widths come from `STATIC_DATA`, not from the DOM

### Negative
- Every state change triggers a full DerivedState recomputation
