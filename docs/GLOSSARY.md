# Glossary

cargo-arc visualises a workspace's module and crate dependencies and detects architecture violations.
This file pins the terms whose everyday meaning is too loose for how it uses them.

On conflict this file wins; code, CLI output and documentation follow.
What the tool does with these things is written elsewhere: checking in [RULES.md](RULES.md), the diagram in [DIAGRAM.md](DIAGRAM.md), implementation in [ARCHITECTURE.md](ARCHITECTURE.md).

*Avoid* means: not as a name for that entry.
The column holds only words that compete with ours because they are established elsewhere, either in the literature or in another entry of this file, and each of them is taken up in the prose under its table with the reason it does not fit.
A word listed there stays correct in its own place: `RepresentativeCycles` replaced an exhaustive elementary-cycle enumeration, and that sentence does not break the column.

## Cycles and clusters

| Term | Definition | Avoid |
|------|------------|-------|
| **Cycle** | A circular dependency between modules, `a -> b -> c -> a`. A closed sequence of nodes. | elementary cycle |
| **Representative cycle** | The cycle that stands in for one edge: the shortest cycle through it, kept once per distinct arc set. Every cycle cargo-arc reports is one of these. | minimal cycle, base cycle |
| **Counted cycle** | A representative cycle, named for the role it plays: it counts toward a tangle's numbers and is what a feedback arc set is measured against. | reported cycle |
| **Unlisted cycle** | A cycle the enumeration never names, because a shorter cycle runs through every one of its edges and is listed in its place. | reported cycle |
| **Cyclic edge** | An edge lying on at least one representative cycle; equivalently, an edge whose endpoints share a non-trivial strongly connected component. The unit the diagram highlights. | back edge |
| **Cluster** | A strongly connected component of the module graph: the maximal set of modules that all reach each other. At least two modules, one or more cycles, never spans crates. | — |
| **Tangle** | The same set as a cluster, named for what a reader sees rather than for its graph property. Structure101: "a set of items that form a cyclic dependency graph at any scope". | — |

*Cycle* and *circular dependency* are two registers for one thing.
*Cycle* is the graph-theoretic name and belongs to the analysis; *circular dependency* is the name in dependency analysis, the domain cargo-arc works in, and is the term for anything a user reads.

Inside that output the line decides which of the two: prose says *circular dependency*, while counts and table cells say *cycle*, because they have to fit beside other numbers and are read as a column rather than a sentence.

*Elementary cycle* names every cycle without a repeated node.
cargo-arc reports a subset of those and never enumerates them all, so the term overstates what is on offer.

*Back edge* belongs to a depth-first traversal, and which edges are back edges depends on where that search started.
A cyclic edge is a property of the graph and holds however it is walked.

A representative cycle represents its edge, and through it the other cycles that cross the edge.
It is the shortest of them, and the shortness describes that one cycle only.

*Minimal* and *minimum* are both taken and both say something else.
In graph theory a minimal cycle carries no chord, and the minimum is the girth, the shortest cycle anywhere in the graph.
This one is neither: shortest relative to one edge, and free to carry a chord, because the shortcut a chord opens need not cross that edge.
*Base cycle* misses a third way.
A cycle basis generates every cycle of the graph and is smaller than one cycle per edge, so the word promises a completeness that is not there.
[ADR-021](adr/021-minimal-cycle-per-edge.md) keeps *minimal cycle* in its title; a decision record states the wording of its own day and is not rewritten.

*Counted cycle* names the role a representative cycle plays once a caller starts tolerating some of them: the cycles a tangle's numbers add up, and the target a feedback arc set is measured against.
*Reported cycle* fits it worse than it looks: a counted cycle can be frozen, and a frozen cycle counts without ever being reported as a violation.

*Unlisted cycle* sits outside the representative-cycle enumeration itself: a shorter cycle runs through every one of its edges and is listed in its place, so the enumeration never names it.
*Reported cycle* fits an unlisted cycle no better — unlisted, it can never be reported at all.

*Cluster* and *tangle* are two registers for one set, the same way *cycle* and *circular dependency* are.
*Cluster* is the graph property and belongs to the analysis, where the underlying term is strongly connected component; *tangle* says how the modules are wound together and is the term for anything a user reads.

A tangle holding exactly one cycle is a *single-cycle tangle*, more than one a *multi-cycle tangle*.
The report is shaped differently for each.
Which dependencies the search takes in, and what it prints, is in [RULES.md](RULES.md#no-cycles).

## Feedback arcs

| Term | Definition | Avoid |
|------|------------|-------|
| **Feedback arc** | An edge whose removal breaks cycles. In a single-cycle tangle every edge is one and removing any of them suffices; in a multi-cycle tangle they come as a set to be removed together. | cut |
| **Feedback arc set** | The edge set whose joint removal breaks every counted cycle. With nothing tolerated it reaches past the enumeration to make the tangle acyclic; with a tolerated cycle it stops at the counted ones. Not unique. | cut set |
| **Traffic** | How many counted cycles run through one edge. Removing it removes all of them. Order-independent, and the basis for ranking feedback arcs. | edge betweenness |
| **Symbol count** | How many distinct symbols cross one edge, each counted once however many import lines carry it. Breaks ties in the traffic ranking, and decides it alone in a single-cycle tangle, where every edge carries the same traffic. | — |

Every feedback arc is a cyclic edge, not the other way round.
*Cyclic edge* states a property of the edge; *feedback arc* is the role it was given in one solution.
That one reads *arc* and the other *edge* follows their sources, the feedback-arc-set literature works on directed graphs and says arc.
Both name the same object, a directed dependency between two modules.

*Cut* and *cut set* mean something else: in graph theory a cut partitions the vertices and the cut set is the edge set between the two halves.
That is connectivity, not cyclicity, and both notions turn up in this tool.

The set is deliberately not the *minimum* feedback arc set, which is NP-hard.
Say *greedy feedback arc set* where the distinction matters.
The report states the set in prose instead of naming it ([RULES.md](RULES.md#no-cycles)).

*Traffic* is a term of this project's own.
*Edge betweenness*, the nearest established word, counts shortest paths rather than cycles.

Nothing calls an edge *thin* or *thick*.
Width is geometry in this tool: an arc's width is how far it bulges, a stroke's width is how the highlight scales it.
An edge carrying few symbols is described by that count, not by a shape.

## Rules and violations

| Term | Definition | Avoid |
|------|------------|-------|
| **Rule** | One named check from `arc-rules.toml`, of type `layers`, `forbidden-dependency` or `no-cycles`. A name is unique across all types. | — |
| **Severity** | How bad breaking a rule is: `error`, `warn`, `ignore`. A property of the rule, not of what it finds. | — |
| **Violation** | One fact a rule established: a dependency, or a cycle, under that rule. A dependency holds whether it is written as one edge or runs through nodes in between, and either way the violation names its two ends. A manifest entry and an import between the same two ends are one dependency, not two. Every violation is in exactly one of the three states below. | finding |
| **Reported** | The state that counts: neither allowed nor frozen. Only reported violations reach the exit code. | — |
| **Allowed** | Permitted by an `except` entry on the rule, permanently and by intent. | whitelisted, ignored |
| **Frozen** | Covered by an `arc-baseline.toml` entry: debt that exists, is tolerated until someone gets to it, and is expected to shrink. | baselined |
| **Silenced** | The genus of allowed and frozen, and what `--show-silenced` lists. Never a state on its own. | suppressed |
| **Baseline** | The set of frozen violations, kept in `arc-baseline.toml` beside the rules file. Only `--generate-baseline` writes it. | — |
| **Diagnostic** | A gap in the configuration rather than in the architecture: a node an exhaustive `layers` rule leaves in no position, a baseline entry that matches nothing, an `except` that matches nothing, a rule pattern that matches nothing, a catch-all layer that holds nothing. | — |
| **Diagnostic level** | Whether the state a diagnostic names is tolerated: `allow`, `warn`, `deny`. | severity |
| **Layer** | One position in a `layers` rule, holding one or more patterns, or `*` for the nodes no other layer holds. Patterns in the same layer sit at the same position. | tier |
| **Exhaustive rule** | A `layers` rule carrying `exhaustive = true`, which claims to sort everything it addresses: its crate patterns claim every workspace crate, its module patterns every module of the crates those patterns reach. Without the field a rule says nothing about what it does not name. | total, complete |
| **Pattern** | A module path with optional wildcards: `domain`, `domain::service`, `domain::*`, `domain::**`, `domain*` (`*` standing for any run of characters within one segment), or a bare `**`. | glob |
| **Scope** | The pattern a `no-cycles` rule searches inside. Not a concept beside pattern, just the name of its role there. | — |

*Allowed* and *frozen* are kept apart because one is meant to stay and the other is meant to shrink.

*Finding* is what Semgrep and Detekt call the violation itself, so it would add a second noun for one thing instead of a distinction.

*Whitelisted* names a mechanism and covers allowed and frozen alike, which is the one line those two words exist to draw.
*Ignored* is taken by the severity: a rule at `ignore` finds nothing, while an allowed violation was found and then permitted.
*Baselined* says an entry sits in the file, not that the debt is meant to shrink.
*Suppressed* is the usual word across linters for hiding a result and invites reading it as a state beside allowed and frozen, which silenced is not.

*Severity* and *diagnostic level* stay separate because they qualify different objects: severity says how bad breaking an intent is, the level says whether a state is acceptable.
A diagnostic is not a violation and carries no severity.

Both axes are configured in one set of words and printed in another.
`error`, `warn`, `ignore`, `allow` and `deny` say what to do with a case; the output names what the run produced, an error or a warning.
So a violation of severity `warn` is printed and counted as a warning, and a diagnostic at level `deny` is printed as an error.
The printed word is neither a fourth value of an axis nor a severity assigned to a diagnostic.
A rule with nothing reported is headed `silenced` in place of that word, which says the block holds nothing that counts, not that silenced is a severity.

*Glob* promises the shell's matching, and what is absent here is character classes, alternation and negation.
`*` is the one wildcard, and it never crosses a `::`.

*Layer* also names an SVG stacking order in the frontend.
That is the ordinary graphics sense and it stays; this entry governs the rule position.
*Tier* is a deployment boundary in the architecture literature, while a layer here is a position in a rule and is matched against module paths.

*Total* and *complete* both overstate the claim: a rule written only from module patterns is exhaustive over the modules of the crates it reaches, and says nothing about the rest of the workspace.
*Exhaustive* is scoped by what the rule addresses, which is the point of the field.

### What a run prints

| Term | Definition | Avoid |
|------|------------|-------|
| **Report** | The blocks on stderr: one per rule that fired, headed by the rule and holding each of its violations, with the locations and the edge or cycle each one found. What a reader goes to for why a run is red. | output |
| **Status** | How one rule came out: `ok`, `WARN` when it produced warnings only, `FAILED` when it produced an error. One line on stdout carries it, per rule and one for the configuration, printed whether or not anything fired. | severity |
| **Location** | The file and line of an import that writes an edge, printed as `--> path:line` and listed in the sidebar. The path is relative to the root of the analyzed workspace; a file outside it stays absolute. | source |

The report and the status lines are split by role, not by audience.
The report says what was found, a status line says how one rule came out.

*Severity* is configured and belongs to the rule; a status is produced and belongs to the run.
The two do not read off each other in either direction: a rule of severity `error` whose violations are all frozen has status `ok`.

*Source* is taken in `rules/engine.rs`, where it names the outgoing end of an edge.

## Symbols and consumers

| Term | Definition | Avoid |
|------|------------|-------|
| **Provider** | A module other modules import symbols from. | — |
| **Consumer** | A module that imports a symbol. A symbol imported by `pub use` is republished, not consumed. | — |
| **Consumer group** | The symbols of one provider that share exactly the same consumers. | cluster |
| **Consumer locality** | How closely a consumer group's consumers sit together in the module tree: one consumer, several under a common ancestor module, or scattered across the crate. | scope |

*Consumer locality* answers one question: could these symbols move closer to the modules that use them?
It is three named cases rather than a measured distance.
The sidebar's wording for the three is in [DIAGRAM.md](DIAGRAM.md#the-sidebar-for-an-arc).

A group is the unit that moves.
Its symbols share one set of consumers, so the group's locality holds for every symbol in it.

*Cluster* and *scope* are both taken in this file: a cluster is a strongly connected component, and a scope is the pattern a `no-cycles` rule searches inside.
Neither has anything to do with who imports a symbol.

## The diagram

| Term | Definition | Avoid |
|------|------------|-------|
| **Arc type** | Which of three dependencies an arc draws: crate-dep, module-dep or re-export. Every arc has exactly one, and it is read off the arc's endpoints and its re-export flag rather than stored. | kind, level |
| **Re-export** | One of the three arc types: an arc whose imports are all `pub use`, so it passes names on rather than depending on them. A single ordinary import behind it makes it a module dependency instead ([ADR-022](adr/022-reexport-edges-tagged-not-dropped.md)). | — |
| **Filter** | One switch over what the diagram shows, offered as a toolbar checkbox. Four cover arcs (crate dependencies, module dependencies, re-exports, cycles), the others cover nodes. | layer |

*Kind* is taken twice over and neither use is this one: it says whether a reference sits in production or in test source, and in cargo it classifies a manifest dependency as normal, dev or build.
Both cut across the arc type, since one pair of crates can carry a production and a test edge of the same type.
*Level* would put the three values in an order they do not have.
A node carries a type in the same sense, so the two read alike.

A filter is not a classification.
Three of the four arc filters select on the arc's type, which is one of crate-dep, module-dep and re-export; the cycles filter selects on a property an arc carries in addition to its type.
An arc can therefore fall under two filters at once.
Which arcs a set of filters leaves visible is in [DIAGRAM.md](DIAGRAM.md#filters).

*Layer* is taken twice over and fits neither: it names a position in a `layers` rule, and in the frontend an SVG stacking container.
All arcs sit in one stacking layer whatever filters cover them, so the two groupings cut across each other.

*Suppressed* names an arc the diagram does not draw because another already covers it: a crate arc that a module arc between the same pair duplicates, or an arc outside the selection in group mode.
That is the rendering sense and it stays.
A violation that was found and then hidden is *silenced*, never suppressed.

*Cluster mode* is what the cycles filter's checkbox turns on in addition to filtering.
It is not a filter and keeps its own name.
