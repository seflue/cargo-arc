# ADR-017: SCC Condensation for Deterministic Cycle Resolution

- **Status:** Active
- **Decided:** 2026-02-11
- **Updated:** 2026-02-13

## Context

Module sorting broke when cross-subtree dependencies formed cycles (e.g. A's child uses B, B's child uses A). These cycles aren't real circular dependencies — they arise from combining two analysis levels (crate-level and module-level edges). But topological sorting can't handle cycles at all, so even these false positives caused non-deterministic output.

Simple heuristics (crate-level propagation, fallback sorting) didn't solve this reliably.

## Decision

Three-step approach:

1. **Break cycles:** Tarjan's SCC (Strongly Connected Components) algorithm groups mutually-dependent nodes into clusters. Collapsing each cluster into a single node produces a cycle-free graph (DAG).

2. **Sort the DAG:** Kahn's algorithm sorts the clusters in dependency order. When multiple clusters are equally valid, alphabetical order breaks the tie (determinism).

3. **Order within clusters:** Each cluster may contain nodes that form a cycle. We try every possible ordering and pick the one where the fewest edges point "upward" (against the reading direction). Edge weights reflect how many cross-subtree dependencies exist in each direction, so asymmetric relationships (A uses B more than the reverse) are reflected correctly. For clusters larger than 8 nodes we fall back to alphabetical order (trying all orderings would be too expensive).

These mixed-edge cycles are resolved silently, without visualizing them — they are analysis artifacts, not architecture problems.

## Rationale

- Condensation mathematically guarantees a cycle-free graph — no heuristic needed
- Kahn's algorithm with alphabetical tiebreaker produces deterministic output independent of internal graph representation (cf. ADR-001: Deterministic Layout)
- Weighted ordering within clusters minimizes visual upward edges, making the diagram easier to read
- Visualizing these false-positive cycles would be misleading

## Consequences

### Positive
- Stable sorting for arbitrary workspace topologies
- Minimal upward edges within cycle clusters

### Negative
- Double SCC computation, though performance overhead is negligible
- Real module-level cycles within a cluster are resolved along with the false positives
- Brute-force ordering limited to clusters of n≤8 nodes (40,320 permutations)

## References

- [Tarjan (1972): "Depth-first search and linear graph algorithms"](https://doi.org/10.1137/0201010), SIAM J. Comput. 1(2):146-160. (SCC algorithm)
- [Kahn (1962): "Topological sorting of large networks"](https://doi.org/10.1145/368996.369025), Comm. ACM 5(11):558-562. (Deterministic topological sort)
- [Minimum Feedback Arc Set](https://en.wikipedia.org/wiki/Feedback_arc_set): NP-hard problem (Karp, 1972). Our intra-cluster ordering is a brute-force solution for the weighted variant — O(n!) justifies the guard at n>8.
