# ADR-008: Enforce Z-Order via Named SVG Layers

- **Status:** Active
- **Decided:** 2026-01-24
- Corrected 2026-10-13: `highlight-shadows` is the third layer, below the highlighted arcs, not the fifth; and `jump-popover-layer` lies above all of them.

## Context

Arc-count labels were obscured by overlapping arcs when multiple edges were highlighted. Dehighlighting did not reliably restore correct layering. The existing `bringToFront()` function moved elements to the end of their parent node indiscriminately — without distinguishing by element type or highlight status.

## Decision

We partition the SVG structure into named `<g>` layers that guarantee a deterministic Z-order. Each visual element belongs to exactly one layer; the LayerManager controls assignment.

Layer order (bottom to top):

1. `base-arcs-layer` — all dependency arcs
2. `base-labels-layer` — arc-count labels
3. `highlight-shadows` — shadow paths for depth effect, behind the highlighted arcs
4. `highlight-arcs-layer` — highlighted arcs
5. `highlight-labels-layer` — labels of highlighted arcs
6. `hitareas-layer` — invisible pointer-event areas
7. `highlight-hitareas-layer` — hitareas for highlighted elements
8. `jump-popover-layer` — the jump popover, which stays under the pointer above every arc and hit area

## Rationale

- SVG renders elements in document order — the layer hierarchy enforces correct Z-order structurally rather than via runtime manipulation
- Highlight operations move elements between base and highlight layers; clearing moves them back — no timing dependencies
- The LayerManager encapsulates layer logic in a dedicated JS module

## Consequences

### Positive
- Z-order is guaranteed by SVG DOM structure, not by CSS or timing
- Labels are always visible above arcs, highlights above non-highlights
- LayerManager is independently testable

### Negative
- All highlight operations must perform layer moves
- New element types require a corresponding layer
