# ADR-017: SCC Condensation for Deterministic Cycle Resolution

- **Status:** Active
- **Decided:** 2026-02-11
- **Updated:** 2026-09-17

## Context

Module sorting broke when cross-subtree dependencies formed cycles (e.g. A's child uses B, B's child uses A). These cycles aren't real circular dependencies — they arise from combining two analysis levels (crate-level and module-level edges). But topological sorting can't handle cycles at all, so even these false positives caused non-deterministic output.

Simple heuristics (crate-level propagation, fallback sorting) didn't solve this reliably.

## Decision

Three-step approach:

1. **Break cycles:** Tarjan's SCC (Strongly Connected Components) algorithm groups mutually-dependent nodes into clusters. Collapsing each cluster into a single node produces a cycle-free graph (DAG).

2. **Sort the DAG:** Kahn's algorithm sorts the clusters in dependency order. When multiple clusters are equally valid, alphabetical order breaks the tie (determinism).

3. **Order within clusters:** The sibling graph is built from the same production subgraph the cycle diagnosis runs on, so a pure re-export edge has no say in the order unless re-exports are included. Where a cycle runs between modules, the diagnosis has already chosen its feedback arcs; the sibling graph counts each of them in the reverse direction, so the sort puts the arc's target above its source and the arc is the edge that points "upward" (against the reading direction). A cluster that is still cyclic after that is a mixed-edge cycle with no cycle between modules. For those we try every possible ordering and pick the one where the least edge weight points upward, weights reflecting how many cross-subtree dependencies exist in each direction. For clusters larger than 8 nodes we fall back to alphabetical order (trying all orderings would be too expensive).

These mixed-edge cycles are resolved silently, without visualizing them — they are analysis artifacts, not architecture problems.

The feedback arcs are reversed rather than dropped: when the rest of a cycle runs outside the parent whose children are being ordered, nothing else at that level constrains the arc's two siblings, and the arc could end up pointing down.

## Rationale

- Condensation mathematically guarantees a cycle-free graph — no heuristic needed
- Kahn's algorithm with alphabetical tiebreaker produces deterministic output independent of internal graph representation (cf. ADR-001: Deterministic Layout)
- Taking the order from the diagnosis makes the picture and the report name the same edges: what points up in a tangle is, as far as the sibling cycles allow, what the report lists as a feedback arc
- Weighted ordering within the remaining clusters minimizes visual upward edges, making the diagram easier to read
- Visualizing these false-positive cycles would be misleading

## Consequences

### Positive
- Stable sorting for arbitrary workspace topologies
- A feedback arc of the report is an upward edge wherever the siblings around it form no mixed-edge cycle of their own
- Minimal upward edges within the remaining cycle clusters

### Negative
- A mixed-edge cycle can remain inside a tangle once its feedback arcs are reversed; there the weight decides, an edge that is no feedback arc can point up, and a feedback arc can point down
- Brute-force ordering limited to clusters of n≤8 nodes (40,320 permutations); above that the alphabet decides, and a feedback arc points either way

## References

- [Tarjan (1972): "Depth-first search and linear graph algorithms"](https://doi.org/10.1137/0201010), SIAM J. Comput. 1(2):146-160. (SCC algorithm)
- [Kahn (1962): "Topological sorting of large networks"](https://doi.org/10.1145/368996.369025), Comm. ACM 5(11):558-562. (Deterministic topological sort)
- [Minimum Feedback Arc Set](https://en.wikipedia.org/wiki/Feedback_arc_set): NP-hard problem (Karp, 1972). The module-level set comes from the greedy cover of the cycle diagnosis; the ordering of what remains is a brute-force solution for the weighted variant — O(n!) justifies the guard at n>8.
