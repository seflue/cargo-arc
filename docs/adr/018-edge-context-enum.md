# ADR-018: EdgeContext Enum for Production and Test Context on Edges

- **Status:** Active
- **Decided:** 2026-02-11

## Context

The graph (ADR-005) did not distinguish between production and test dependencies. A boolean `is_test` flag would have sufficed, but tests are not just a filter toggle — we want to filter by them, style them differently, and later distinguish by kind (unit, integration).

## Decision

We introduce `EdgeContext` as a nested enum: `Production` and `Test(TestKind)`. We convert the edge variants `CrateDep` and `ModuleDep` to struct variants with a `context: EdgeContext` field. When a module references the same target from both production and test code, two separate edges are created (dedup key: `(full_target(), context)`).

`EdgeContext` is independent of `EffectiveDepKind`, which operates at the Cargo.toml level — `EdgeContext` operates at source code level.

## Rationale

- A boolean would have prevented coarse/fine matching: `Test(_)` matches any test context, `Test(Unit)` only unit tests — this requires an enum
- Future contexts (benchmarks, examples) only need a new variant

## Consequences

### Positive
- `topo_sort` and `detect_cycles` filter on production edges — test dependencies no longer distort the sort order
- Differentiated visualization of test and production edges possible

### Negative
- Every pipeline stage must account for context filtering
- Edge count increases for modules with mixed dependencies on the same target
