# Glossary

cargo-arc visualizes a workspace's module and crate dependencies and detects architecture violations.
This file defines the terms it uses in a specific sense.

See also [RULES.md](RULES.md), [ARC_DIAGRAM.md](ARC_DIAGRAM.md), [HOTSPOT_MAP.md](HOTSPOT_MAP.md) and [ARCHITECTURE.md](ARCHITECTURE.md).

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
| **Cycle** | A chain of direct dependencies between modules that leads back to the module it started from, such as `a -> b -> c -> a`. |
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

A counted cycle is not always *reported* (see [Rules and violations](#rules-and-violations)).
In a tangle whose cycles are all frozen, every cycle counts and none is reported.
An unlisted cycle is never reported.

The code says *cluster*, and output a user reads says *tangle*.

A tangle with exactly one cycle is a *single-cycle tangle*, and a tangle with more is a *multi-cycle tangle*.
[RULES.md](RULES.md#no-cycles) describes which dependencies the search includes and how the report differs between the two.

## Feedback arcs

| Term | Definition |
|------|------------|
| **Feedback arc** | An edge in a feedback arc set. In a single-cycle tangle, any one edge of the cycle is a feedback arc set by itself. |
| **Feedback arc set** | A set of edges whose removal together breaks every counted cycle of a tangle. In a tangle where some but not all cycles are frozen, it breaks only the counted cycles. In any other tangle it makes the tangle acyclic, which can take more edges than the counted cycles need. A tangle can have more than one feedback arc set. |
| **Traffic** | The number of counted cycles that run through one edge. Removing the edge breaks all of them. Traffic does not depend on which other edges are removed first. Feedback arcs are ranked by traffic. |
| **Symbol count** | The number of distinct symbols that cross one edge. A symbol counts once, even when several imports on the edge carry it. All bare imports on an edge together add one. A symbol imported by `pub use` does not count, unless every import on the edge is a `pub use`. The symbol count breaks ties in the traffic ranking. In a single-cycle tangle every edge has the same traffic, so the symbol count alone decides the order. |

Every feedback arc is a cyclic edge.
*Feedback arc* is the established name from the literature on directed graphs, which says *arc* for an edge (see [Nodes and edges](#nodes-and-edges)).

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

A *whitelist* is a list of exceptions, and the word fits allowed and frozen violations alike, so it does not tell them apart.
An *ignored* rule has severity `ignore` and is never checked, while an allowed violation was found and then permitted.
*Baselined* says only that a violation has an entry in `arc-baseline.toml`.
Linters usually call a hidden result *suppressed*, and cargo-arc calls it *silenced*.
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

| Term | Definition |
|------|------------|
| **Report** | The blocks that `check` writes to stderr, one per rule that fired. A block is headed by the rule and lists its violations, each with its dependency or tangle and its locations. |
| **Status** | The outcome of one rule in a run: `ok`, `WARN` when the rule produced only warnings, or `FAILED` when it produced an error. `check` prints one status line on stdout for each checked rule and one for the configuration, whether or not anything fired. |
| **Location** | A file and a line, such as the place of an import that creates an edge. The report prints a location as `--> path:line`, and the sidebar of the arc diagram lists it. The path is relative to the root of the analyzed workspace, and a file outside the workspace keeps its absolute path. |

Severity is configured on a rule, and a status is the result of a run.
The severity does not determine the status: a rule of severity `error` whose violations are all frozen has status `ok`.

In the code, the *source* of an edge is the node it starts from, not a location.

## Hotspots

| Term | Definition |
|------|------------|
| **Code lines** | Tokei's `code` count for a file: its source lines without comments and blank lines. |
| **Container** | A circle that holds other circles: the workspace, a crate, or a module with children. The file that declares it appears as a leaf inside it. |
| **Leaf** | The circle of a single file. Size and commits are measured on leaves. |
| **Hotspot** | One of the top-N files of the workspace, ranked by code lines times commit count. A hotspot is always a leaf. |
| **Hotspot map** | The view of a workspace as nested circles, sized by code lines and colored by commit activity. |
| **Label band** | The strip at the top of a container that stays free for the container's label. The container's children are shifted down by its height. |

*LOC* and *lines of code* can include comments and blank lines; code lines never do.

*Hot file* names a property that a file has or lacks, and a hotspot is a rank among all files.

*Group* and *folder* suggest a directory in the file system, and a container can also be a crate or the workspace.

A *node* is a crate or module in the dependency graph (see [Nodes and edges](#nodes-and-edges)), and a leaf is the circle of one file in the hotspot map.

A *treemap* nests rectangles.
cargo-arc tried one and draws nested circles instead, which show the nesting depth better but leave space unused.

## Symbols and consumers

| Term | Definition |
|------|------------|
| **Provider** | A module that other modules import symbols from. |
| **Consumer** | A module that imports a symbol from a provider in production code. A module that imports a symbol only by `pub use` republishes it and is not its consumer. |
| **Consumer group** | The symbols of one provider that have exactly the same consumers. |
| **Consumer locality** | How close the consumers of a consumer group sit to each other in the module tree. It has three cases: a single consumer, several consumers under a common ancestor module that does not contain the provider, or consumers spread across the crate. |

Consumer locality shows whether the symbols of a group could move closer to the modules that use them.
[ARC_DIAGRAM.md](ARC_DIAGRAM.md#the-sidebar-for-an-arc) gives the sidebar's wording for each case.

A consumer group is the unit that can move.
Its symbols share one set of consumers, so the group's locality holds for each of them.

In this file, a *cluster* is a strongly connected component (see [Cycles and clusters](#cycles-and-clusters)), and a *scope* is the pattern a `no-cycles` rule searches inside (see [Rules and violations](#rules-and-violations)).

## The arc diagram

| Term | Definition |
|------|------------|
| **Arc diagram** | The view of a workspace as a tree of crates and modules, with arcs for the dependencies between them. `cargo arc` renders it, and `cargo arc ui` serves it beside the hotspot map. |
| **Arc type** | The category of dependency an arc draws: crate-dep, module-dep or re-export. Every arc has exactly one arc type. cargo-arc derives it from the arc's endpoints and its re-export flag and does not store it. |
| **Re-export** | The arc type of an arc whose imports are all `pub use`, so that it passes names on without depending on them. A single ordinary import on the arc makes it a module-dep instead ([ADR-022](adr/022-reexport-edges-tagged-not-dropped.md)). |
| **Filter** | A checkbox in the **View** menu that shows or hides part of the diagram. Four filters cover arcs: crate dependencies, module dependencies, re-exports and circular dependencies. The others cover nodes. |
| **Reading order** | The top-to-bottom sequence of the nodes under one parent. A node comes before the nodes it depends on. Inside a tangle no order can do that for every edge, and the layout picks the order with the least dependency weight pointing up. A tangle of more than eight nodes under one parent is ordered alphabetically instead. |
| **Upward edge** | An edge whose target sits above its source in the reading order. Inside a tangle some edge must be upward. Outside a tangle, an upward edge occurs only where two subtrees each hold a module that uses a module of the other: a cycle between the subtrees without a cycle between modules. The diagram draws upward edges in their own style. |

In this section and in [ARC_DIAGRAM.md](ARC_DIAGRAM.md), *diagram* alone means the arc diagram.
Where a text also covers the hotspot map, say *arc diagram*.

*Kind* has two other meanings in this project.
It says whether a reference is in production or in test code, and in Cargo it classifies a manifest dependency as normal, dev or build.
Both are independent of the arc type: one pair of crates can have a production edge and a test edge of the same arc type.
*Level* would suggest an order among the three arc types, and they have none.

Three of the four arc filters select on the arc type.
The circular-dependencies filter selects on whether the arc is a cyclic edge, so an arc can fall under two filters at once.
[ARC_DIAGRAM.md](ARC_DIAGRAM.md#filters) describes which arcs a set of filters leaves visible.

A *layer* is a position in a `layers` rule or an SVG stacking order (see [Rules and violations](#rules-and-violations)).
All arcs sit in one SVG stacking layer, whichever filters cover them.

A *suppressed* arc is one the diagram does not draw: a crate arc between two crates whose modules a module arc already connects, or, in group mode, an arc that does not touch the selected node.

The checkbox of the circular-dependencies filter also turns on *cluster mode*, which is not a filter.

The reading order follows a levelization in Lakos's sense, and Structure101 calls an edge against it a *feedback (upward) dependency*.
A node's *level* is the one Lakos defines: 0 for what lies outside the workspace, 1 for a node that depends on nothing inside it, and otherwise one more than the highest level among the nodes it depends on (Vol. I, section 1.10).
Levels are not positions in the reading order, and several nodes can share a level.
Lakos gives every member of a cycle the highest level the cycle spans (footnote 128), so levels cannot order the nodes of a tangle.
The *diagnostic level* of a rules file is unrelated and is always written with *diagnostic*.

Whether an edge is a *back edge* depends on where a depth-first search starts (see [Cycles and clusters](#cycles-and-clusters)).
Whether an edge is upward depends only on the reading order, which a reader can see in the diagram.
In package managers, the *reverse dependencies* of a package are the packages that depend on it.

An upward edge inside a tangle is a cyclic edge.
It need not be a feedback arc: the layout weighs module edges between sibling subtrees, the cycle search looks at cycles between modules, and the two can pick different edges.
Deriving the reading order from the feedback arc set is open work.
