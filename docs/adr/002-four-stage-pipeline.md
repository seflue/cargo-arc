# ADR-002: Structure Processing as Four-Stage Pipeline

- **Status:** Active
- **Decided:** 2026-01-20

## Context

The path from workspace source code to interactive SVG encompasses conceptually different tasks: parse source code, model dependencies as a graph, compute positions, render SVG. Separating these into stages allows each to be tested independently and backends to be swapped.

## Decision

We structure processing as a linear four-stage pipeline with defined intermediate formats:

1. Analyze: Source code -> Modules + Dependencies (`CrateInfo`, `ModuleInfo`)
2. Graph: -> Nodes + Edges (Node/Edge enums, petgraph)
3. Layout: -> Positions (LayoutIR with x/y coordinates)
4. Render: -> SVG (with embedded CSS + JS + metadata)

## Rationale

- Each stage has defined input/output types
- Backends are swappable (syn vs HIR in the analysis step, see ADR-013)
- Independently testable (graph tests need no files, layout tests need no parser)

## Consequences

### Positive
- Clear responsibilities per module

### Negative
- Intermediate formats must be kept consistent when changes occur
