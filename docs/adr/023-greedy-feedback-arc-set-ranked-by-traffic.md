# ADR-023: Build the Feedback Arc Set Greedily and Rank It by Traffic

- **Status:** Active
- **Decided:** 2026-07-14
- **Related:** ADR-021 (representative cycles), ADR-022 (re-export edges)

## Context

A tangle holds one or more cycles, and ADR-021 gives every cyclic edge a representative cycle.
For a tangle with more than one cycle, the report lists edges whose removal together breaks its cycles: a feedback arc set.

Finding the minimum feedback arc set is NP-hard.
The literature approximates two targets: the fewest edges, or the lowest total reference weight, which is how Sonargraph's cycle break-up works.

The report prints the set as a list, so its edges need an order.

Some cycles can be frozen by the baseline.
A tangle can then hold frozen and counted cycles side by side.

## Decision

The feedback arc set of a tangle is a greedy cover of its counted cycles.
Each step takes the edge that lies on the most cycles still open.
Where every cycle of the tangle is counted, the set is then checked against the tangle itself, and the cover continues until no cycle is left.
Where the tangle mixes frozen and counted cycles, the set covers the counted cycles and stops there.
The set is not the minimum, and nothing searches for the minimum.

The list ranks the feedback arcs by traffic, the number of counted cycles through an edge.
Among edges of equal traffic, the lower symbol count comes first.

The ranking is a diagnosis of where a tangle is held together most tightly.
It does not say which edge to remove first.

## Rationale

- Refactoring a tangle happens in steps, and a few edges more than the minimum change nothing for the reader. The NP-hardness concerns the minimum, not a small set that is checked to work.
- One check after the cover is enough to confirm the set. Recomputing the tangle after every pick would cost more and confirm the same.
- Traffic does not depend on which edges were picked before, so each number on the list is true by itself and the same graph gives the same list. A marginal count, the cycles an edge breaks that earlier picks left open, would change with the order of the picks.
- The fewest-edges target assumes that one design change per edge, such as inverting a dependency, breaks a cycle. In the cases checked on a large workspace, the fix was to move a symbol to another module, which that assumption does not describe.
- What removing an edge costs depends on the kind of item that crosses it. A string constant moves at no cost, and the methods of a type defined in the parent module cannot move without splitting the type. A reference count does not tell these cases apart, so the reference-weighted target would order by a number that does not measure the cost. The symbol count has the same weakness, which is why it only breaks ties.
- In a tangle with frozen cycles, covering the rest of the tangle could add an edge that lies on no counted cycle.

## Consequences

- The traffic numbers of one list do not add up to the tangle's cycle count, because cycles share edges.
- A tangle can have several valid feedback arc sets. The report states that its set is enough, not that it is the only one or the smallest.
- The head of the list is the edge the most cycles run through. Two modules that model one concept can meet there, and then no edge between them can be removed. The output therefore does not present the first entry as a recommendation.
- The symbol count orders edges only within equal traffic. A better cost measure would change the order below the head of the list and seldom the head itself.
- In a tangle that mixes frozen and counted cycles, a cycle that is not on the list can still stand once every listed edge is gone.
- If the check ever ended with a cycle left standing, that cycle would appear again on the next run.
- That a reference count points away from the removable edge is an argument and has not been measured. The symbol count counts symbols, not import lines, so a measurement is now possible.
- Which symbol to move where is a question about symbols, not edges, and lies outside this decision.

## References

- [Glossary: Feedback arcs](../GLOSSARY.md#feedback-arcs): feedback arc set, traffic, symbol count, counted cycle.
- [RULES.md: `no-cycles`](../RULES.md#no-cycles): how the report prints the list.
- `src/diagnose/clusters.rs`: the cover, the check and the ranking.
