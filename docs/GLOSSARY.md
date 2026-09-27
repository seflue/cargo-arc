# Glossary

cargo-arc visualizes a workspace's module and crate dependencies and detects architecture violations.
This file pins the terms whose everyday meaning is too loose for how it uses them.

On conflict this file wins; code, CLI output and documentation follow.
What the tool does with these things is written elsewhere: checking in [RULES.md](RULES.md), the arc diagram in [ARC_DIAGRAM.md](ARC_DIAGRAM.md), the hotspot map in [HOTSPOT_MAP.md](HOTSPOT_MAP.md), implementation in [ARCHITECTURE.md](ARCHITECTURE.md).

Where another word names the same thing, or the same word means something else elsewhere, the prose under the table says so.

## Nodes and edges

| Term | Definition |
|------|------------|
| **Node** | In Rust, the crates of the workspace and the modules within them are the nodes of the dependency graph. Modules can be nested at any depth. `--externals` adds the external crates the workspace depends on to the graph. Patterns match only nodes inside the workspace. |
| **Edge** | The directed connection from one node to another that the first depends on directly. A transitive dependency runs through other nodes and is not an edge. |
| **Qualified node name** | A node's path starting at its crate: `storage` is the crate, `storage::pool` a module in it. `check` writes both ends of a `layers` or `forbidden-dependency` violation as qualified node names. |
| **Manifest edge** | The edge from one crate to another that the first declares in its `Cargo.toml`. No import in the source code creates it. |
| **Bare import** | An import of a module itself instead of an item in it, such as `use storage::pool;`. |

Graph theory calls a node a *vertex*.
The literature on directed graphs calls an edge an *arc*.
In the arc diagram, an arc is the curve that draws a dependency (see [Arc type](#the-arc-diagram)).

## Cycles and clusters

| Term | Definition |
|------|------------|
| **Cycle** | A closed sequence of modules in which each module depends on the next, such as `a -> b -> c -> a`. |
| **Representative cycle** | The shortest cycle through one edge. Where several edges have the same shortest cycle, cargo-arc keeps it once. Every cycle cargo-arc reports is a representative cycle. |
| **Counted cycle** | A representative cycle that a tangle's numbers count and its feedback arc set breaks. In a tangle with at least one cycle that is not frozen, these are the cycles that are not frozen. In a tangle whose cycles are all frozen, they are all of its cycles. |
| **Unlisted cycle** | A cycle that is not the representative cycle of any of its edges. For each of its edges, cargo-arc lists a shorter or equally short cycle instead. |
| **Cyclic edge** | An edge that lies on at least one cycle. Its two ends are in the same cluster. The arc diagram highlights cyclic edges. |
| **Cluster** | A strongly connected component of the module graph with at least two modules: the maximal set of modules that all reach each other. A cluster holds one or more cycles and never spans crates. |
| **Tangle** | The same set of modules as a cluster. Structure101 defines a tangle as "a set of items that form a cyclic dependency graph at any scope". |

*Circular dependency* is the word in dependency analysis for a cycle.
The code says *cycle*.
Output a user reads says *circular dependency* in sentences, and *cycle* in counts and table cells.

Graph theory calls a cycle without a repeated node an *elementary cycle*.
Every representative cycle is elementary, but cargo-arc never lists all elementary cycles.

In a depth-first search, a *back edge* leads from a node to one of its ancestors in the search tree.
Which edges are back edges depends on where the search starts.
Whether an edge is cyclic does not.

A representative cycle stands in for every cycle through its edge.
In graph theory, a *minimal cycle* has no chord, an edge that joins two of its nodes without being part of it.
A representative cycle can have a chord, as long as the shorter cycle that the chord creates does not pass through its edge.
The *minimum* cycle is the shortest cycle in the whole graph, and its length is the *girth*.
A representative cycle is the shortest only among the cycles through its edge.
[ADR-021](adr/021-minimal-cycle-per-edge.md) calls a representative cycle a *minimal cycle* in its title.

A *cycle basis* is a set of cycles from which every cycle of the graph can be built.
cargo-arc does not choose the representative cycles to form one.

*Reported* is a state of a violation (see [Rules and violations](#rules-and-violations)).
A counted cycle in a tangle whose cycles are all frozen is not reported, and an unlisted cycle is never reported.

The code says *cluster*, and output a user reads says *tangle*.

A tangle with exactly one cycle is a *single-cycle tangle*, and a tangle with more is a *multi-cycle tangle*.
[RULES.md](RULES.md#no-cycles) describes which dependencies the search includes and how the report differs between the two.

## Feedback arcs

| Term | Definition |
|------|------------|
| **Feedback arc** | An edge in a feedback arc set. In a single-cycle tangle, any one edge of the cycle is a feedback arc set by itself. |
| **Feedback arc set** | A set of edges whose removal together breaks every counted cycle of a tangle. In a tangle where some but not all cycles are frozen, it breaks only the counted cycles. In any other tangle it makes the tangle acyclic, which can take more edges than the counted cycles need. A tangle can have more than one feedback arc set. |
| **Traffic** | The number of counted cycles that run through one edge. Removing the edge breaks all of them. Traffic counts every one of them, whichever other edges are removed first. Feedback arcs are ranked by traffic. |
| **Symbol count** | The number of distinct symbols that cross one edge. A symbol counts once, even when several imports on the edge carry it. All bare imports on an edge together add one. A symbol imported by `pub use` does not count, unless every import on the edge is a `pub use`. The symbol count breaks ties in the traffic ranking. In a single-cycle tangle every edge has the same traffic, so the symbol count alone decides the order. |

Every feedback arc is a cyclic edge.
*Arc* is the directed-graph word for an edge (see [Nodes and edges](#nodes-and-edges)), and *feedback arc* is the established name.

In graph theory, a *cut* splits the nodes into two parts, and the *cut set* is the set of edges between them.
A cut concerns connectivity, and a feedback arc set concerns cycles.

cargo-arc builds the feedback arc set greedily.
It is not the *minimum* feedback arc set, which is NP-hard to find.
Where the difference matters, say *greedy feedback arc set*.
The report describes the set in a sentence without naming it ([RULES.md](RULES.md#no-cycles)).

*Traffic* is a term of this project.
*Edge betweenness* is the nearest established term, and it counts the shortest paths through an edge instead of cycles.

Removing a feedback arc moves code, and Lakos names the two directions it can go.
*Escalation* moves mutually dependent functionality into a component above both modules, an existing one or a new one.
*Demotion* moves common functionality into a component below both.
They are two of the nine levelization techniques in Lakos, *Large-Scale C++* Vol. I, section 3.5.
cargo-arc does not use either word yet; a hint that proposes a move should use them.

## Module roles

| Term | Definition |
|------|------------|
| **Vocabulary** | The types, constants and errors that a module holds for its descendants to read. A cycle whose edges toward an ancestor carry only vocabulary is intended and is not debt. |
| **Facade** | A module that is the entry point to its subtree for all code outside it. It declares and re-exports its children and may call into them, but holds nothing they read back. Its edges point down only, so it closes no cycle on its own. |
| **Container module** | A facade with no code of its own, only `mod` declarations and `pub use`. |
| **Prelude** | A module that holds nothing of its own and re-exports names defined elsewhere, so that other code can import them with one glob import. All its edges are re-exports. |

In Rust a parent module is often both the facade of its subtree and the holder of the subtree's vocabulary.
A cycle between such a parent and its children has two halves: the children read the vocabulary upward, and the parent calls downward.
[RULES.md](RULES.md#allow) describes the `allow` entries that permit the upward half.

In this section, *up* and *down* follow the module tree.
For a facade, Lakos's levels give the same direction.
For vocabulary they give the opposite one: a module that others depend on sits low in his hierarchy, so a parent that holds its subtree's vocabulary sits below its own children there.

Lakos calls types that pass through function boundaries, such as a date or an allocator, *vocabulary types*, and places them low (Vol. I, section 0.4).
The idea is the same, but his term names a property of a type, and *vocabulary* here names what a module holds.
He has no word for a parent that holds its children's types.

*Facade* has the meaning Lakos and the Gang of Four give it: one interface over a whole subsystem.
Lakos places a facade above what it wraps (Vol. I, section 0.7), and the levelization technique that builds one is *escalating encapsulation*.
Lakos also calls a facade a *wrapper*.
In Rust a wrapper is usually a newtype around a single type.

With a facade, a detail behind it can change without its clients changing their code, which Lakos calls *encapsulation*.
His *insulation* also spares the clients a recompile (Vol. I, section 3.11.1).
In Rust the crate is the unit of compilation, so a module facade cannot spare a recompile to a client in the same crate.

A prelude re-exports names like a facade, but the names come from outside its subtree, often from its parent, so its edges point up.
A re-export passes a name on without depending on it, so a prelude depends on nothing.

In C4, a *container* is an application or a data store, a unit that runs on its own.
A workspace with several binaries holds several C4 containers, and no module is one.
Say *container module* for a module and *C4 container* for the deployable unit.
In the hotspot map, a *container* is a circle that holds other circles (see [Hotspots](#hotspots)).

Lakos calls a unit that only groups components a *package*; it is not a component itself.
In Cargo, a *package* is what a `Cargo.toml` describes.
A Lakos component, a header with its implementation file, corresponds to a module file here.

## Rules and violations

| Term | Definition |
|------|------------|
| **Rule** | One named check in `arc-rules.toml`, of type `layers`, `forbidden-dependency` or `no-cycles`. Its name is unique across all types. |
| **Severity** | How bad it is to break a rule: `error`, `warn` or `ignore`. Severity belongs to the rule, and all its violations share it. |
| **Violation** | A dependency or a tangle that breaks a rule. The dependency can be direct or transitive. A manifest entry and an import between the same two ends are one dependency. Every violation is in exactly one of the three states below. |
| **Reported** | The state of a violation that is neither allowed nor frozen. Only reported violations affect the exit code. |
| **Allowed** | Permitted by an `allow` entry on the rule, permanently and on purpose. An entry names an edge that runs against the order the rule's writer has in mind, and the entries of a rule together declare that order. |
| **Frozen** | Covered by an entry in `arc-baseline.toml`. A frozen violation is debt that is tolerated until someone fixes it and is expected to shrink. |
| **Silenced** | Allowed or frozen. `--show-silenced` lists silenced violations. Silenced is not a state of its own. |
| **Baseline** | The set of frozen violations, kept in `arc-baseline.toml` beside the rules file. Only `--generate-baseline` writes it. |
| **Diagnostic** | A gap in the configuration: a node that an exhaustive `layers` rule leaves in no position, an `except` entry that matches no node, a baseline entry that matches nothing, a baseline entry that freezes more symbols than the edge still carries, an `allow` entry that matches nothing, `allow` entries that put nodes above each other in a circle, a rule pattern that matches nothing, or a catch-all layer that holds nothing. |
| **Diagnostic level** | Whether a run tolerates the gap a diagnostic names: `allow`, `warn` or `deny`. |
| **Layer** | One position in a `layers` rule. It holds one or more patterns, or `*` for the nodes that no other layer holds. Patterns in the same layer share its position. |
| **Exhaustive rule** | A `layers` rule with `exhaustive = true`, which claims to sort everything it addresses. Its crate patterns claim every workspace crate, and its module patterns claim every module of the crates they reach. A rule without the field says nothing about the nodes it does not name. |
| **Module path pattern** | A module path with optional wildcards, such as `domain`, `domain::service`, `domain::*`, `domain::**`, `domain*` or a bare `**`. Inside one segment, `*` stands for any run of characters. *Pattern* alone means a module path pattern unless the text says otherwise. |
| **Dependency pattern** | A named list of `allow` entries under `[dependency-patterns]`, which a rule's `allow` list refers to by name. It only selects dependencies; the `allow` list that names it allows them. |
| **Scope** | The module path pattern in the `scope` field of a `no-cycles` rule. The rule searches only the edges whose two ends the pattern matches. |

Semgrep and Detekt call a violation a *finding*.

A *whitelist* is a list of exceptions, and the word fits allowed and frozen violations alike.
An *ignored* rule has severity `ignore` and is never checked, while an allowed violation was found and then permitted.
*Baselined* says only that a violation has an entry in `arc-baseline.toml`.
Linters usually call a hidden result *suppressed*.
In the arc diagram, a suppressed arc is one the diagram does not draw (see [The arc diagram](#the-arc-diagram)).

A diagnostic is not a violation and has no severity.

The configuration and the output use different words.
Severity and diagnostic level are configured as `error`, `warn`, `ignore`, `allow` and `deny`.
The output calls what a run produced an *error* or a *warning*: a violation of severity `warn` is printed and counted as a warning, and a diagnostic at level `deny` is printed as an error.
A rule's block with no reported violation is headed `silenced` instead.

A shell *glob* also has character classes, alternation and negation.
A module path pattern has none of them.
Its `*` never crosses a `::`, and `**` stands only as the whole pattern or as its last segment.

In the frontend code, a *layer* is also an SVG stacking order.
In the architecture literature, a *tier* is a deployment boundary.

*Total* and *complete* would claim the whole workspace.
An exhaustive rule written only from module patterns covers the modules of the crates it reaches and says nothing about other crates.

### What a run prints

| Term | Definition | Avoid |
|------|------------|-------|
| **Report** | The blocks on stderr: one per rule that fired, headed by the rule and holding each of its violations, with the locations and the edge or cycle each one found. What a reader goes to for why a run is red. | output |
| **Status** | How one rule came out: `ok`, `WARN` when it produced warnings only, `FAILED` when it produced an error. One line on stdout carries it, per rule and one for the configuration, printed whether or not anything fired. | severity |
| **Location** | A file and a line. An import that writes an edge has one, printed as `--> path:line` and listed in the sidebar; the path is relative to the root of the analyzed workspace, a file outside it stays absolute. | source |

The report and the status lines are split by role, not by audience.
The report says what was found, a status line says how one rule came out.

*Severity* is configured and belongs to the rule; a status is produced and belongs to the run.
The two do not read off each other in either direction: a rule of severity `error` whose violations are all frozen has status `ok`.

*Source* is taken in `rules/engine.rs`, where it names the outgoing end of an edge.

## Hotspots

| Term | Definition | Avoid |
|------|------------|-------|
| **Code lines** | Tokei's `code` count for a file: source lines, excluding comments and blank lines. | LOC, lines of code |
| **Container** | A circle holding other circles: a workspace, a crate, or a module with children. Its own declaring file appears as a leaf inside it. | group, folder |
| **Leaf** | A single file's circle: the unit size and commits are measured on. | node |
| **Hotspot** | One of the top-N files, workspace-wide, ranked by code lines times commit count. A leaf only, never a container. | hot file |
| **Hotspot map** | The nested-circle view of a workspace: size by code lines, color by commit activity. | treemap |
| **Label band** | The strip a container keeps free at its top for its own label. Its children are shifted down by it. | — |

*LOC* and *lines of code* both leave open whether comments and blanks are counted; this tool's count never does.

*Hot file* fits the same idea but not the plural sense the ranking needs: a hotspot is a rank, not just a property a file has or lacks.

*Group* and *folder* both suggest a filesystem directory; a container can be a crate or a workspace as well as a module, and is drawn as a circle, not a tree row.

*Node* already names one crate or module in the dependency graph (see Dependencies); a leaf is a circle in the hotspot map, a narrower thing.

*Treemap* names the layout this tool tried and rejected: nested circles read the nesting depth better, at the cost of wasted space.

## Symbols and consumers

| Term | Definition | Avoid |
|------|------------|-------|
| **Provider** | A module other modules import symbols from. | — |
| **Consumer** | A module that imports a symbol. A symbol imported by `pub use` is republished, not consumed. | — |
| **Consumer group** | The symbols of one provider that share exactly the same consumers. | cluster |
| **Consumer locality** | How closely a consumer group's consumers sit together in the module tree: one consumer, several under a common ancestor module, or scattered across the crate. | scope |

*Consumer locality* answers one question: could these symbols move closer to the modules that use them?
It is three named cases rather than a measured distance.
The sidebar's wording for the three is in [ARC_DIAGRAM.md](ARC_DIAGRAM.md#the-sidebar-for-an-arc).

A group is the unit that moves.
Its symbols share one set of consumers, so the group's locality holds for every symbol in it.

*Cluster* and *scope* are both taken in this file: a cluster is a strongly connected component, and a scope is the pattern a `no-cycles` rule searches inside.
Neither has anything to do with who imports a symbol.

## The arc diagram

| Term | Definition | Avoid |
|------|------------|-------|
| **Arc diagram** | The view of a workspace as a tree of crates and modules, with arcs for the dependencies between them. What `cargo arc` renders; `cargo arc ui` serves it beside the hotspot map. | — |
| **Arc type** | Which of three dependencies an arc draws: crate-dep, module-dep or re-export. Every arc has exactly one, and it is read off the arc's endpoints and its re-export flag rather than stored. | kind, level |
| **Re-export** | One of the three arc types: an arc whose imports are all `pub use`, so it passes names on rather than depending on them. A single ordinary import behind it makes it a module dependency instead ([ADR-022](adr/022-reexport-edges-tagged-not-dropped.md)). | — |
| **Filter** | One switch over what the diagram shows, offered as a toolbar checkbox. Four cover arcs (crate dependencies, module dependencies, re-exports, cycles), the others cover nodes. | layer |
| **Reading order** | The top-to-bottom sequence of the nodes under one parent. A node comes before the nodes it depends on; inside a tangle, where no order can do that for every edge, the layout takes the one that leaves the least dependency weight pointing up. | level, layer |
| **Upward edge** | An edge whose target sits above its source in the reading order. Inside a tangle some edge has to; outside one it occurs only where two subtrees each hold a module using the other's, a cycle between the subtrees with none between modules. Drawn in its own style. | back edge, reverse dependency |

*Diagram* alone is short for the arc diagram only where the hotspot map is out of reach, as in this section and in [ARC_DIAGRAM.md](ARC_DIAGRAM.md).
Where both views are in reach, it names neither.

*Kind* is taken twice over and neither use is this one: it says whether a reference sits in production or in test source, and in cargo it classifies a manifest dependency as normal, dev or build.
Both cut across the arc type, since one pair of crates can carry a production and a test edge of the same type.
*Level* would put the three values in an order they do not have.
A node carries a type in the same sense, so the two read alike.

A filter is not a classification.
Three of the four arc filters select on the arc's type, which is one of crate-dep, module-dep and re-export; the cycles filter selects on a property an arc carries in addition to its type.
An arc can therefore fall under two filters at once.
Which arcs a set of filters leaves visible is in [ARC_DIAGRAM.md](ARC_DIAGRAM.md#filters).

*Layer* is taken twice over and fits neither: it names a position in a `layers` rule, and in the frontend an SVG stacking container.
All arcs sit in one stacking layer whatever filters cover them, so the two groupings cut across each other.

*Suppressed* names an arc the diagram does not draw because another already covers it: a crate arc that a module arc between the same pair duplicates, or an arc outside the selection in group mode.
That is the rendering sense and it stays.
A violation that was found and then hidden is *silenced*, never suppressed.

*Cluster mode* is what the cycles filter's checkbox turns on in addition to filtering.
It is not a filter and keeps its own name.

The reading order follows a levelization in Lakos's sense, and Structure101 calls an edge against it a *feedback (upward) dependency*.
A node's *level* is Lakos's: 0 for what lies outside the workspace, 1 for a node that depends on nothing inside it, and otherwise one more than the highest level among the nodes it depends on (Vol. I, section 1.10).
Levels explain the reading order but are not positions in it: several nodes can share a level, and Lakos gives every member of a cycle the highest level the cycle spans (footnote 128), so inside a tangle a level cannot order the nodes.
The *diagnostic level* of a rules file is a different thing and always carries its qualifier.
*Layer* is a position in a `layers` rule.

*Back edge* is relative to a traversal (see Cycles and clusters); an upward edge is relative to the reading order, which the layout fixes, so a reader can check it against the picture.
*Reverse dependency* is taken by package managers, where it names the packages that depend on a given one.

An upward edge inside a tangle is a cyclic edge.
Whether it is also a feedback arc is not settled by definition today: the layout weighs module edges between sibling subtrees, the diagnosis covers cycles between modules, and the two can name different edges.
Deriving the reading order from the feedback arc set is open work.
