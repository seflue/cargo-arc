# ADR-021: Detect Cycles via the Minimal Cycle per Edge

- **Status:** Active
- **Decided:** 2026-07-10
- **Supersedes:** ADR-019

## Context

Johnson's algorithm (ADR-019) enumerates *all* elementary cycles. It is output-sensitive: on a sufficiently dense dependency graph the number of overlapping cycles explodes into the billions and hangs the tool.

## Decision

For each edge in a non-trivial strongly connected component, we compute the **minimal cycle** through that edge (shortest path back from the edge's target to its source, plus the edge) and deduplicate by arc set. This bounded set replaces Johnson's enumeration everywhere: diagram highlighting, the legacy `--check`, and the `no-cycles` rule.

Cycle *breaking* for layout is unaffected — `stable_toposort` still uses SCC condensation (ADR-017), which remains in use.

The pipeline data model (`cycle_ids: Vec<usize>` per edge, `cycles: {nodes, arcs}`) from ADR-019 is unchanged; only the set of cycles that populates it changes.

## Rationale

- Output is bounded by the edge count (≤ e per SCC before dedup) — polynomial, no cap, no hang, deterministic.
- Every cyclic edge lies on at least its own minimal cycle, so highlighting stays complete.
- Each cycle shown is a genuine *directed* dependency cycle and is minimal, so it reads clearly.
- The number of minimal cycles an edge is shared across is a free break-candidate signal (*traffic* in the [glossary](../GLOSSARY.md)).

## Consequences

### Positive

- Fixes the dense-graph hang without a truncating limit.
- Selecting an edge highlights a handful of minimal cycles instead of, potentially, thousands.

### Negative

- The emitted set is a representative sample, not the complete set of elementary cycles: two distinct cycles through the same edge that are equally short collapse to one deterministic pick.

## References

- Supersedes [ADR-019](./019-elementary-cycles-via-johnsons-algorithm.md).
- Design: `memories/specs/2026-07-10-ca-0340-minimal-cycles-design.org`
