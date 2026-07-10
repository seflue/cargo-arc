# ADR-005: Model Hierarchy and Dependencies in Unified Graph

- **Status:** Active
- **Decided:** 2026-01-20

## Context

The dependency graph must model two relationship types: hierarchy (crate contains modules) and dependencies (module uses another module/crate). Separate data structures would complicate traversal and layout computation.

## Decision

We model both in a unified petgraph graph with a typed Node enum (`Crate|Module`) and Edge enum (`Contains|CrateDep|ModuleDep`). Contains edges connect parent to child nodes.

## Rationale

- Unified traversal for layout, collapse, and cycle detection
- Visibility propagation (collapsing a crate hides all modules) operates on the same edges
- petgraph provides ready-made algorithms (Tarjan SCC, topological sort)

## Consequences

### Positive
- Layout algorithm operates on one graph instead of two data structures
- Cycle detection finds cycles across hierarchy boundaries

### Negative
- Contains edges are not real dependencies and must be filtered during analyses
