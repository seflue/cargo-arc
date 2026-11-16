# ADR-017: SCC Condensation for Deterministic Cycle Resolution

- **Status:** Active
- **Decided:** 2026-02-11
- **Updated:** 2026-11-16
- **Related:** ADR-001 (deterministic layout), ADR-023 (feedback arc set)

## Context

The diagram lists the nodes under one parent from top to bottom, a node before the nodes it depends on.
A topological sort gives such a sequence only for a graph without cycles.

Siblings form cycles in two ways.
Inside a tangle, modules use each other.
Outside a tangle, two sibling subtrees can each hold a module that uses a module of the other, without a cycle between modules.

Until 2026-11-16 the order inside a tangle came from the feedback arc set of the cycle diagnosis.
A cycle that was left among up to eight siblings was ordered by a search over every order, and a larger one alphabetically.

## Decision

The reading order of the modules follows the level order of the tangles.
An upward edge counts in the reverse direction, so the modules give the order no cycle.

Siblings that still form a cycle are condensed into one node, and the condensed graph is sorted topologically.
The siblings of a cycle stand together.
Among the siblings free to come next, and among the siblings of a cycle, the one whose subtree has the higher mean level comes first, then the one first by name.

The feedback arc set of ADR-023 is unchanged and is a different set of edges.

Packages have no level.
They keep the search over every order for a cycle of up to eight packages and the alphabetical order above that.

## Rationale

- Condensation guarantees a graph without cycles, so the sort needs no heuristic.
- The level order is the same on every run and the name decides every remaining choice, so the same graph gives the same diagram (ADR-001).
- The level order covers a tangle of any size. The search over every order stopped at eight siblings, and above that the position of a row said nothing about its dependencies.
- With the levels as the source, the diagram, `query tangles` and `query edges` name the same edges as upward.
- The level order weighs an edge by its symbol count. ADR-023 rejects a reference weight for ranking the feedback arcs, because the count does not tell how hard the code behind an edge is to change. The level order makes no statement about that. It decides which of two modules that use each other the diagram shows as the user, and takes the one that uses more symbols of the other.
- An upward edge is reversed where it could be dropped. When the rest of its cycle runs outside the parent whose children are being ordered, nothing else constrains its two siblings.
- A cycle between subtrees without a cycle between modules comes from reading module edges at the level of subtrees. The diagram does not draw it as a cycle.

## Consequences

- An upward edge of the diagram need not be a feedback arc of `check`, and a feedback arc need not be upward.
- An upward edge can point down in the diagram, and another edge can point up. The row of a parent module stays above its children whatever their levels, and one position has to serve a whole subtree.
- The order of siblings changes outside tangles too, because siblings that do not depend on each other now come by mean level.
- Another level order can leave as few symbols on the edges against it and give another diagram.
- A subtree with modules on many levels has a mean that describes none of them.

## References

- [Tarjan (1972): "Depth-first search and linear graph algorithms"](https://doi.org/10.1137/0201010), SIAM J. Comput. 1(2):146-160.
- [Kahn (1962): "Topological sorting of large networks"](https://doi.org/10.1145/368996.369025), Comm. ACM 5(11):558-562.
- [Glossary: Levels](../GLOSSARY.md#levels): level order, level, upward edge.
