# ADR-005: Model Hierarchy and Dependencies in Unified Graph

- **Status:** Active
- **Decided:** 2026-01-20
- Corrected 2026-10-13: the node variants are `Package|Module|ExternalPackage` and the edge variants `Contains|PackageDep|ModuleDep|DevDep`. A package is what the language adapter delivers (ADR-029).

## Context

The dependency graph must model two relationship types: hierarchy (crate contains modules) and dependencies (module uses another module/crate). Separate data structures would complicate traversal and layout computation.

## Decision

We model both in a unified petgraph graph with a typed Node enum (`Package|Module|ExternalPackage`) and Edge enum (`Contains|PackageDep|ModuleDep|DevDep`). Contains edges connect parent to child nodes.
A package is what the language adapter delivers as one (ADR-029); for Rust it is a crate.
`DevDep` edges are not drawn; reachability uses them.

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
