# ADR-018: EdgeContext Enum for Production and Test Context on Edges

- **Status:** Active
- **Decided:** 2026-02-11
- Corrected 2026-10-13: `EdgeContext` is a struct whose field `kind` holds the enum `UsageKind`. The edge variant is `PackageDep`, not `CrateDep`, and the Cargo-level type is `EffectiveDependencyKind`. Production edges are selected by `ArcGraph::production_subgraph` and by the layout, not by the functions `topo_sort` and `detect_cycles`, which do not exist.

## Context

The graph (ADR-005) did not distinguish between production and test dependencies. A boolean `is_test` flag would have sufficed, but tests are not just a filter toggle — we want to filter by them, style them differently, and later distinguish by kind (unit, integration).

## Decision

We introduce `EdgeContext`, a struct whose field `kind` is the nested enum `UsageKind` with the variants `Production` and `Test(TestKind)`.
The edge variants `PackageDep` and `ModuleDep` are struct variants with a `context: EdgeContext` field.
When a module references the same target from both production and test code, two separate edges are created (dedup key: `(full_target(), context.kind)`).

`EdgeContext` is independent of `EffectiveDependencyKind`, which operates at the Cargo.toml level — `EdgeContext` operates at source code level.

## Rationale

- A boolean would have prevented coarse/fine matching: `Test(_)` matches any test context, `Test(Unit)` only unit tests — this requires an enum
- Future contexts (benchmarks, examples) only need a new variant

## Consequences

### Positive
- The cycle analysis (`ArcGraph::production_subgraph`) and the layout's sort order are based on production edges, so test dependencies do not distort them
- Differentiated visualization of test and production edges possible

### Negative
- Every pipeline stage must account for context filtering
- Edge count increases for modules with mixed dependencies on the same target
