# Glossary

cargo-arc visualises a workspace's module and crate dependencies and detects
architecture violations. This file pins the terms whose everyday meaning is too
loose for how it uses them.

On conflict this file wins; code, CLI output and documentation follow.

*Avoid* means: not as a name for that entry. The column holds only words that
compete with ours because they are established elsewhere, either in the
literature or in another entry of this file, and each of them is taken up in the
prose under its table with the reason it does not fit. A word listed there stays
correct in its own place: `RepresentativeCycles` replaced an exhaustive
elementary-cycle enumeration, and that sentence does not break the column.

## Cycles and clusters

| Term | Definition | Avoid |
|------|------------|-------|
| **Cycle** | A circular dependency between modules, `a -> b -> c -> a`. A closed sequence of nodes. | elementary cycle |
| **Representative cycle** | The cycle that stands in for one edge: the shortest cycle through it, kept once per distinct arc set. Every cycle cargo-arc reports is one of these. | minimal cycle, base cycle |
| **Cyclic edge** | An edge lying on at least one representative cycle; equivalently, an edge whose endpoints share a non-trivial strongly connected component. The unit the diagram highlights. | back edge |
| **Cluster** | A strongly connected component of the module graph: the maximal set of modules that all reach each other. At least two modules, one or more cycles, never spans crates. | — |
| **Tangle** | The same set as a cluster, named for what a reader sees rather than for its graph property. Structure101: "a set of items that form a cyclic dependency graph at any scope". | — |

*Cycle* and *circular dependency* are two registers for one thing. *Cycle* is
the graph-theoretic name and belongs to the analysis; *circular dependency* is
the name in dependency analysis, the domain cargo-arc works in, and is the term
for anything a user reads.

*Elementary cycle* names every cycle without a repeated node. cargo-arc reports
a subset of those and never enumerates them all, so the term overstates what is
on offer.

*Back edge* belongs to a depth-first traversal, and which edges are back edges
depends on where that search started. A cyclic edge is a property of the graph
and holds however it is walked.

A representative cycle represents its edge, and through it the other cycles that
cross the edge. It is the shortest of them, but the shortness only describes the
one cycle: after deduplication an edge is listed with every kept cycle it lies
on, not only with its own shortest.

*Minimal* and *minimum* are both taken and both say something else. In graph
theory a minimal cycle carries no chord, and the minimum is the girth, the
shortest cycle anywhere in the graph. This one is neither: shortest relative to
one edge, and free to carry a chord, because the shortcut a chord opens need not
cross that edge. *Base cycle* misses a third way. A cycle basis generates every
cycle of the graph and is smaller than one cycle per edge, so the word promises
a completeness that is not there. [ADR-021](adr/021-minimal-cycle-per-edge.md)
keeps *minimal cycle* in its title; a decision record states the wording of its
own day and is not rewritten.

*Cluster* and *tangle* are two registers for one set, the same way *cycle* and
*circular dependency* are. *Cluster* is the graph property and belongs to the
analysis, where the underlying term is strongly connected component; *tangle*
says how the modules are wound together and is the term for anything a user
reads.

A tangle holding exactly one cycle is a *single-cycle tangle*, more than one a
*multi-cycle tangle*. That split decides the shape of the report: one cycle is
written out in full with the edge carrying the fewest symbols, several get the
ranked edge list.

cargo-arc searches for circular dependencies between modules. A dependency
between crates never takes part, and neither does an import written in a test
or in a build script.

A re-export does not count as a real dependency: a module that writes `pub use`
only passes the name on. By default such an arc takes no part in the search, so
it can never be one leg of a circle. `--include-reexports` puts those arcs back
in. An arc counts as a re-export only if every import behind it is a `pub use`;
a single ordinary import makes it a real dependency
([ADR-022](adr/022-reexport-edges-tagged-not-dropped.md)).

## Feedback arcs

| Term | Definition | Avoid |
|------|------------|-------|
| **Feedback arc** | An edge whose removal breaks cycles. In a single-cycle tangle every edge is one and removing any of them suffices; in a multi-cycle tangle they come as a set to be removed together. | cut |
| **Feedback arc set** | The edge set whose joint removal makes the tangle acyclic. Not unique. | cut set |
| **Traffic** | How many cycles run through one edge. Removing it removes all of them. Order-independent, and the basis for ranking feedback arcs. | edge betweenness |
| **Symbol count** | How many distinct symbols cross one edge, each counted once however many import lines carry it. Breaks ties in the traffic ranking, and decides it alone in a single-cycle tangle, where every edge carries the same traffic. | — |

Every feedback arc is a cyclic edge, not the other way round. *Cyclic edge*
states a property of the edge; *feedback arc* is the role it was given in one
solution. That one reads *arc* and the other *edge* follows their sources, the
feedback-arc-set literature works on directed graphs and says arc. Both name the
same object, a directed dependency between two modules.

*Cut* and *cut set* mean something else: in graph theory a cut partitions the
vertices and the cut set is the edge set between the two halves. That is
connectivity, not cyclicity, and both notions turn up in this tool.

The set is deliberately not the *minimum* feedback arc set, which is NP-hard.
Say *greedy feedback arc set* where the distinction matters. The report states
the set in prose ("every cycle contains at least one of these edges") instead of
naming it.

*Traffic* is a term of this project's own. *Edge betweenness*, the nearest
established word, counts shortest paths rather than cycles.

Nothing calls an edge *thin* or *thick*. Width is geometry in this tool: an
arc's width is how far it bulges, a stroke's width is how the highlight scales
it. An edge carrying few symbols is described by that count, not by a shape.

### Severity axes of a tangle

How bad a tangle is has more than one dimension, and no single number carries
all three:

| Axis | Number | Question |
|------|--------|----------|
| Extent | modules | how much code is stuck in it |
| Intensity | cycles | how tightly it is woven |
| Feedback arcs | set size | how many edges have to go |

None of the three is a cost estimate, and the arc count least of all: greedy
cover makes it an upper bound rather than the minimum, and the edges are not
equal. Dropping a re-export that only forwards is close to free; inverting a
dependency is not. It says how many places have to be touched, not how much work
that is.

## Rules and violations

| Term | Definition | Avoid |
|------|------------|-------|
| **Rule** | One named check from `arc-rules.toml`, of type `layers`, `forbidden-dependency` or `no-cycles`. A name is unique across all types. | — |
| **Severity** | How bad breaking a rule is: `error`, `warn`, `ignore`. A property of the rule, not of what it finds. | — |
| **Violation** | One fact a rule established: an edge, or a cycle, under that rule. Every violation is in exactly one of the three states below. | finding |
| **Reported** | The state that counts: neither allowed nor frozen. Only reported violations reach the exit code. | — |
| **Allowed** | Permitted by an `except` entry on the rule, permanently and by intent. | whitelisted, ignored |
| **Frozen** | Covered by an `arc-baseline.toml` entry: debt that exists, is tolerated until someone gets to it, and is expected to shrink. | baselined |
| **Silenced** | The genus of allowed and frozen, and what `--show-silenced` lists. Never a state on its own. | suppressed |
| **Baseline** | The set of frozen violations, kept in `arc-baseline.toml` beside the rules file. Only `--generate-baseline` writes it. | — |
| **Diagnostic** | A gap in the configuration rather than in the architecture: a crate no layer sorts, a baseline entry that matches nothing, an `except` that matches nothing. | — |
| **Diagnostic level** | Whether the state a diagnostic names is tolerated: `allow`, `warn`, `deny`. | severity |
| **Layer** | One position in a `layers` rule, holding one or more patterns. Patterns in the same layer sit at the same position. | tier |
| **Pattern** | A module path with optional wildcards: `domain`, `domain::service`, `domain::*`, `domain::**`, or a bare `**`. | glob |
| **Scope** | The pattern a `no-cycles` rule searches inside. Not a concept beside pattern, just the name of its role there. | — |

*Allowed* and *frozen* are kept apart because one is meant to stay and the other
is meant to shrink. Under `no-cycles` an allowed edge is removed before the
search, so a cycle through it never forms.

*Finding* is what Semgrep and Detekt call the violation itself, so it would add
a second noun for one thing instead of a distinction.

*Whitelisted* names a mechanism and covers allowed and frozen alike, which is
the one line those two words exist to draw. *Ignored* is taken by the severity:
a rule at `ignore` finds nothing, while an allowed violation was found and then
permitted. *Baselined* says an entry sits in the file, not that the debt is
meant to shrink. *Suppressed* is the usual word across linters for hiding a
result and invites reading it as a state beside allowed and frozen, which
silenced is not.

*Severity* and *diagnostic level* stay separate because they qualify different
objects: severity says how bad breaking an intent is, the level says whether a
state is acceptable. A diagnostic is not a violation and carries no severity.

Both axes are configured in one set of words and printed in another. `error`,
`warn`, `ignore`, `allow` and `deny` say what to do with a case; the output
names what the run produced, an error or a warning. So a violation of severity
`warn` is printed and counted as a warning, and a diagnostic at level `deny` is
printed as an error. The printed word is neither a fourth value of an axis nor a
severity assigned to a diagnostic.

The order patterns are written in within one layer says nothing. Wildcards cut
at `::` only, so there is no prefix glob such as `domain_*`.

*Layer* also names an SVG stacking order in the frontend. That is the ordinary
graphics sense and it stays; this entry governs the rule position. *Tier* is a
deployment boundary in the architecture literature, while a layer here is a
position in a rule and is matched against module paths.

## Symbols and consumers

| Term | Definition | Avoid |
|------|------------|-------|
| **Provider** | A module other modules import symbols from. | — |
| **Consumer** | A module that imports a symbol. A symbol imported by `pub use` is republished, not consumed. | — |
| **Consumer group** | The symbols of one provider that share exactly the same consumers. | cluster |
| **Consumer locality** | How closely a consumer group's consumers sit together in the module tree: one consumer, several under a common ancestor module, or scattered across the crate. | scope |

*Consumer locality* answers one question: could these symbols move closer to the
modules that use them? It is three named cases rather than a measured distance,
and it is what the sidebar renders as "only used by", "used under" and "widely
used".

A group is the unit that moves. Its symbols share one set of consumers, so the
group's locality holds for every symbol in it.

*Cluster* and *scope* are both taken in this file: a cluster is a strongly
connected component, and a scope is the pattern a `no-cycles` rule searches
inside. Neither has anything to do with who imports a symbol.

## The diagram

| Term | Definition | Avoid |
|------|------------|-------|
| **Arc type** | Which of three dependencies an arc draws: crate-dep, module-dep or re-export. Every arc has exactly one, and it is read off the arc's endpoints and its re-export flag rather than stored. | kind, level |
| **Filter** | One switch over what the diagram shows, offered as a toolbar checkbox. Four cover arcs (crate dependencies, module dependencies, re-exports, cycles), the others cover nodes. | layer |

*Kind* is taken twice over and neither use is this one: it says whether a
reference sits in production or in test source, and in cargo it classifies a
manifest dependency as normal, dev or build. Both cut across the arc type, since
one pair of crates can carry a production and a test edge of the same type.
*Level* would put the three values in an order they do not have. A node carries
a type in the same sense, so the two read alike.

An arc is shown when at least one arc filter covering it is on and the node
filters leave both its endpoints standing. Both sides write the same
`hidden-by-filter` class, so neither can override the other.

A filter is not a classification. Three of the four arc filters select on the
arc's type, which is one of crate-dep, module-dep and re-export; the cycles
filter selects on a property an arc carries in addition to its type. An arc can
therefore fall under two filters at once.

*Layer* is taken twice over and fits neither: it names a position in a `layers`
rule, and in the frontend an SVG stacking container. All arcs sit in one
stacking layer whatever filters cover them, so the two groupings cut across each
other.

*Suppressed* names an arc the diagram does not draw because another already
covers it: a crate arc that a module arc between the same pair duplicates, or an
arc outside the selection in group mode. That is the rendering sense and it
stays. A violation that was found and then hidden is *silenced*, never
suppressed.

The cycles filter switches the visibility of cyclic edges. Its checkbox
additionally turns on *cluster mode*, which widens hovering, highlighting and
styling from the edge to the whole cluster. That mode is not a filter and keeps
its own name.
