# ADR-019: Detect Elementary Cycles via Johnson's Algorithm

- **Status:** Active
- **Decided:** 2026-02-16

## Context

Tarjan's SCC algorithm (ADR-017) groups all mutually-reachable nodes into a single component. When multiple overlapping cycles share nodes, SCC merges them into one large group — hiding which edges actually form distinct loops.

## Decision

We replace SCC-based cycle detection for visualization with Johnson's elementary cycle algorithm. Each elementary cycle is detected and displayed individually. Edges and nodes can belong to multiple cycles.

The data model changes from `cycle_id: Option<usize>` to `cycle_ids: Vec<usize>` through the entire pipeline.

SCC condensation (ADR-017) remains in use for topological sorting, where merging cycles is the correct behavior.

## Rationale

- Elementary cycles show the actual circular dependency paths, not just the connected component
- Overlapping cycles become individually visible and navigable
- Johnson's algorithm runs in O((n+e)(c+1)) where c is the number of elementary circuits — efficient for the sparse graphs typical of Rust workspaces

## Consequences

### Positive
- Each cycle is independently selectable and highlightable
- Users can distinguish overlapping cycles that share edges

### Negative
- Multiple cycle memberships increase data volume (`Vec<usize>` per edge/node instead of `Option<usize>`)
- Cycle count can be large in highly connected subgraphs

## References

- [Johnson (1975): "Finding all the elementary circuits of a directed graph"](https://doi.org/10.1137/0204007), SIAM J. Comput. 4(1):77-84.
