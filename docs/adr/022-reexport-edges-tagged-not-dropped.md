# ADR-022: Tag Re-Export Edges, Don't Drop Them

- **Status:** Active
- **Decided:** 2026-07-14
- **Related:** ADR-018 (edges carry context), ADR-020 (consumer-side re-export resolution)

## Context

A producer-side `pub use child::X` compiles to a real dependency edge parent→child, but it *republishes* a name rather than expressing behavioral coupling. Previously the analyzer treated such an edge like any behavioral `use`, so a module that merely re-exports a child appeared to depend on it.

The edge is real but of a different *kind*, and several consumers want to treat it specially, each differently:

- the diagram wants to **show** re-export structure, distinct from real dependencies;
- layering rules want to **not** flag a pure forwarding `pub use` as, say, "domain → infra";
- cycle analysis wants to **exclude** it, because a cycle that exists only through a re-export edge is idiomatic, not coupling.

The question is how to represent this kind so each consumer can decide independently.

## Decision

Keep producer-side re-export edges in the graph and tag them with a distinct kind rather than dropping them. Provenance is recorded per reference and aggregated per edge: an edge is a re-export edge only when *all* of its references stem from `pub use`. A mixed edge, with at least one behavioral use, is a logic edge.

Analyses derive the view they need (for example a logic subgraph without pure re-export edges); the raw graph keeps every edge.

## Rationale

- Dropping is lossy and buys nothing for cycle detection. A pure re-export edge only ever lies on cycles that are idiomatic by definition, meaning they vanish once re-export edges are removed. Removing the edge from the graph would therefore hide no cycle that the default view reports. Cycle correctness is **not** a reason to keep the edge.
- What actually needs the edge is elsewhere: the diagram can only render structure that exists, and layering rules can only special-case a forwarding `pub use` if the edge carries its kind.
- One tagged graph with derived views is simpler than maintaining several pre-filtered edge stores.
- This extends the provenance-on-edges idea of ADR-018 (Production/Test/Build) with an orthogonal dimension. It is distinct from ADR-020, which resolves the *target* of a consumer-side import rather than classifying a producer-side edge.

## Consequences

### Positive

- The diagram can show re-export arcs, layering can treat forwarding specially, and the raw graph never loses information.

### Negative

- The domain model, parser, and edge aggregation gain a dimension.
- All-or-nothing per edge: adding a single behavioral `use` next to a `pub use` flips the whole edge to logic and can make a cycle reappear — action at a distance that is not obvious from the code.
- The re-export flag is aggregated in two places (reference dedup and edge construction); both must stay consistent.
- The same `pub use` participates in ADR-020 (consumer-side resolution) and this classification; the two must be reasoned about together.

### New context (handled outside this ADR)

- Now that edges are classified, each analysis and view must choose whether to include re-exports. Those are product decisions (cycle analysis excludes them by default, `--include-reexports` opts back in) recorded with the tool's behavior, not here.

## References

- Extends [ADR-018](./018-edge-context-enum.md).
- Distinct from [ADR-020](./020-resolve-reexports-for-true-dependencies.md).
