# ADR-002: Structure Processing as Four-Stage Pipeline

- **Status:** Active
- **Decided:** 2026-01-20
- Corrected 2026-10-13: stage 1 is the analysis of a language adapter and delivers `Analysis`, not `CrateInfo`. Backends are swapped at the adapter (ADR-029). The cycle analysis runs between Graph and Layout.

## Context

The path from workspace source code to interactive SVG encompasses conceptually different tasks: parse source code, model dependencies as a graph, compute positions, render SVG. Separating these into stages allows each to be tested independently and backends to be swapped.

## Decision

We structure processing as a linear four-stage pipeline with defined intermediate formats:

1. Analyze: Source code -> Packages + Module trees + Externals (`Analysis`, delivered by the language adapter, ADR-029)
2. Graph: -> Nodes + Edges (Node/Edge enums, petgraph)
3. Layout: -> Positions (LayoutIR with x/y coordinates)
4. Render: -> SVG (with embedded CSS + JS + metadata)

Between Graph and Layout, the cycle analysis runs on the graph, and the layout takes its result.

## Rationale

- Each stage has defined input/output types
- Backends are swappable at the language adapter (ADR-029); syn vs HIR is the Rust case, see ADR-013
- Independently testable (graph tests need no files, layout tests need no parser)

## Consequences

### Positive
- Clear responsibilities per module

### Negative
- Intermediate formats must be kept consistent when changes occur
