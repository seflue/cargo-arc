# ADR-024: Key a Baseline Entry on an Edge and Its Symbols

- **Status:** Active
- **Decided:** 2026-08-07
- **Related:** ADR-021 (representative cycles), ADR-023 (frozen and counted cycles)

## Context

`arc-baseline.toml` freezes violations that a workspace tolerates for now and expects to shrink.
A `layers` or `forbidden-dependency` violation runs on one edge.
A `no-cycles` violation is a tangle, and the report shows a tangle through its representative cycles, the shortest cycle through each of its edges (ADR-021).

Which cycle is the representative of an edge depends on the other edges of the tangle.
An edge added to a tangle can give many other edges a new representative cycle, while no cycle has gone away.

An edge carries the symbols one module imports from the other.
The symbols on a frozen edge can grow in number while the edge stays the same.

An `allow` entry permits an edge under a rule for good.

## Decision

A baseline entry is an edge, named by the qualified node names of its two ends, and the set of symbols tolerated on it.
The entry also records whether bare imports on the edge are tolerated.
Every rule type uses this one form, and no entry names a cycle.

Under `no-cycles`, every cyclic edge of a tangle gets an entry, not only its feedback arcs.
An edge is frozen when its entry names every symbol that crosses it now.
A cycle is frozen when each of its edges is.

A renamed symbol counts as one symbol gone and one symbol new.
Nothing pairs the two.

## Rationale

- A baseline that misses a change for the worse is no use as an architecture test. Tolerating such a change on purpose is what an `allow` entry is for. The candidates for the key are measured against these two points.
- A cycle is a derived witness. A new edge elsewhere in the tangle can replace it, which orphans the entry and shows the tangle as new, although no violation was fixed.
- An edge without symbols would stay stable. It would say what an `allow` entry for the edge already says, in a second file. It would also let a growing symbol set pass.
- A count of symbols per edge catches growth but lets a swap pass. Removing one coupling and adding another keeps the count. The report could then not name the new coupling, and a count that went up invites raising the number in the file.
- A set of symbols catches growth and swap, because a new symbol is not in the set either way. The analysis knows no unit finer than the symbol name.
- With entries only for the feedback arcs, an edge added to a frozen tangle would lie in no entry, and nothing would check it.
- The package name stays in the qualified node name. A module such as `util` exists in several packages, and without the package an entry could match an edge that nobody froze. A wrong match tolerates what nobody tolerated, and that costs more than the entries a package rename orphans.
- A heuristic for renames, such as "as many symbols gone as new on the same edge", would be wrong in the swap case, which is the case the key exists for. A symbol has no identity across a rename other than its name.

## Consequences

- The key is the format of a file that lives in the repositories of the workspaces being checked. Introducing it made every cycle-keyed baseline invalid, and changing it again would do the same. Regenerating the baseline repairs such a file.
- Whether a tangle is frozen depends only on the entries of its edges. A change of representative cycle can change what the report shows, and it cannot change the verdict.
- Renaming a symbol on a frozen edge turns the check red until the baseline is regenerated. That is the price of catching growth and swap. The documentation states it, so that the first red run after a rename does not read as a bug.
- An entry stays wider than the edge until the baseline is regenerated. A symbol that no longer crosses the edge is reported as a warning. If the symbol comes back before then, the check passes. Every generated baseline has this gap.
- The file grows. A tangle has at least as many cyclic edges as representative cycles, and each entry lists symbols. Each line in it states a fact about the source code.
- A bare import hides no symbol. What the code then uses through the imported module crosses the same edge as a named symbol.

## References

- [Glossary: Rules and violations](../GLOSSARY.md#rules-and-violations): baseline, frozen.
- [RULES.md: The baseline](../RULES.md#the-baseline): the file and what a run reports about it.
- `src/rules/baseline.rs`: the entry and the file.
- `src/rules/engine.rs`: freezing the cyclic edges of a tangle.
