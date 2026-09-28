# Glossary

cargo-arc visualizes a workspace's module and crate dependencies and detects architecture [violations](#violation).
This file defines the terms it uses in a specific sense.

See also [RULES.md](RULES.md), [ARC_DIAGRAM.md](ARC_DIAGRAM.md), [HOTSPOT_MAP.md](HOTSPOT_MAP.md) and [ARCHITECTURE.md](ARCHITECTURE.md).

## Nodes and edges

| Term | Definition |
|------|------------|
| **<a id="node">Node</a>** | In Rust, the crates of the workspace and the modules within them are the nodes of the dependency graph. Modules can be nested at any depth. `--externals` adds the external crates the workspace depends on to the graph. [Patterns](#module-path-pattern) match only nodes inside the workspace. |
| **<a id="edge">Edge</a>** | The directed connection from one [node](#node) to another that the first depends on directly. A transitive dependency runs through other nodes and is not an edge. |
| **<a id="qualified-node-name">Qualified node name</a>** | A [node](#node)'s path starting at its crate: `storage` is the crate, `storage::pool` a module in it. `check` writes both ends of a `layers` or `forbidden-dependency` [violation](#violation) as qualified node names. |
| **<a id="manifest-edge">Manifest edge</a>** | The [edge](#edge) from one crate to another that the first declares in its `Cargo.toml`. No import in the source code creates it. |
| **<a id="bare-import">Bare import</a>** | An import of a module itself instead of an item in it, such as `use storage::pool;`. |

Graph theory calls a [node](#node) a *vertex*.
The literature on directed graphs calls an [edge](#edge) an *arc*.
In the [arc diagram](#arc-diagram), an arc is the curve that draws a dependency (see [Arc type](#arc-type)).

## Cycles and clusters

| Term | Definition |
|------|------------|
| **<a id="cycle">Cycle</a>** | A chain of direct dependencies between modules that leads back to the module it started from, such as `a -> b -> c -> a`. |
| **<a id="cluster">Cluster</a>** | A strongly connected component of the module graph with at least two modules: the maximal set of modules that all reach each other. A cluster holds one or more [cycles](#cycle) and never spans crates. |
| **<a id="tangle">Tangle</a>** | The same set of modules as a [cluster](#cluster). Structure101 [defines a tangle](https://www.sonarsource.com/structure101/docs/java/studio/Content/restructure101/tangles.html) as "a set of items that form a cyclic dependency graph at any scope …". |
| **<a id="representative-cycle">Representative cycle</a>** | The shortest [cycle](#cycle) through one [edge](#edge). Several edges can share the same representative cycle. Every cycle cargo-arc reports is a representative cycle. |
| **<a id="counted-cycle">Counted cycle</a>** | A [representative cycle](#representative-cycle) of a [tangle](#tangle) that is not [frozen](#frozen). When every cycle of a tangle is frozen, all of them are counted, so that the tangle still has a [feedback arc set](#feedback-arc-set) that shows how to reduce the frozen debt. [Traffic](#traffic) and the feedback arc set consider only counted cycles. |
| **<a id="unlisted-cycle">Unlisted cycle</a>** | A [cycle](#cycle) that is not the [representative cycle](#representative-cycle) of any of its [edges](#edge). For each of its edges, cargo-arc lists a shorter or equally short cycle instead. |
| **<a id="cyclic-edge">Cyclic edge</a>** | An [edge](#edge) that lies on at least one [cycle](#cycle). Its two ends are in the same [cluster](#cluster). The [arc diagram](#arc-diagram) highlights cyclic edges. |

*Circular dependency* is the word in dependency analysis for a [cycle](#cycle).
The code says *cycle*.
Output a user reads says *circular dependency* in sentences, and *cycle* in counts and table cells.

The code says *cluster*, and output a user reads says *tangle*.

A [tangle](#tangle) with exactly one [cycle](#cycle) is a *single-cycle tangle*, and a tangle with more is a *multi-cycle tangle*.
[RULES.md](RULES.md#no-cycles) describes which dependencies the search includes and how the [report](#report) differs between the two.

Graph theory calls a [cycle](#cycle) without a repeated [node](#node) an *elementary cycle*.
Every [representative cycle](#representative-cycle) is elementary, but cargo-arc never lists all elementary cycles.

A [representative cycle](#representative-cycle) stands in for every [cycle](#cycle) through its [edge](#edge).
In graph theory, a *minimal cycle* has no chord, an edge that joins two of its [nodes](#node) without being part of it.
A representative cycle can have a chord, as long as the shorter cycle that the chord creates does not pass through its edge.
The *minimum* cycle is the shortest cycle in the whole graph, and its length is the *girth*.
A representative cycle is the shortest only among the cycles through its edge.
[ADR-021](adr/021-minimal-cycle-per-edge.md) calls a representative cycle a *minimal cycle* in its title.

A *cycle basis* is a set of [cycles](#cycle) from which every cycle of the graph can be built.
cargo-arc does not choose the [representative cycles](#representative-cycle) to form one.

A [counted cycle](#counted-cycle) is not always [reported](#reported).
In a [tangle](#tangle) whose [cycles](#cycle) are all [frozen](#frozen), every cycle counts and none is reported.
An [unlisted cycle](#unlisted-cycle) is never reported.

In a depth-first search, a *back edge* leads from a [node](#node) to one of its ancestors in the search tree.
Which [edges](#edge) are back edges depends on where the search starts.
Whether an edge is [cyclic](#cyclic-edge) does not.

## Feedback arcs

| Term | Definition |
|------|------------|
| **<a id="feedback-arc">Feedback arc</a>** | An [edge](#edge) in a [feedback arc set](#feedback-arc-set). In a [single-cycle tangle](#tangle), any one edge of the [cycle](#cycle) is a feedback arc set by itself. |
| **<a id="feedback-arc-set">Feedback arc set</a>** | A set of [edges](#edge) whose removal together breaks every [counted cycle](#counted-cycle) of a [tangle](#tangle). In a tangle where some but not all [cycles](#cycle) are [frozen](#frozen), it breaks only the counted cycles. In any other tangle it makes the tangle acyclic, which can take more edges than the counted cycles need. A tangle can have more than one feedback arc set. |
| **<a id="traffic">Traffic</a>** | The number of [counted cycles](#counted-cycle) that run through one [edge](#edge). Removing the edge breaks all of them. Traffic does not depend on which other edges are removed first. [Feedback arcs](#feedback-arc) are ranked by traffic. |
| **<a id="symbol-count">Symbol count</a>** | The number of distinct symbols that cross one [edge](#edge). A symbol counts once, even when several imports on the edge carry it. All [bare imports](#bare-import) on an edge together add one. A symbol imported by `pub use` does not count, unless every import on the edge is a `pub use`. The symbol count breaks ties in the [traffic](#traffic) ranking. In a [single-cycle tangle](#tangle) every edge has the same traffic, so the symbol count alone decides the order. |

Every [feedback arc](#feedback-arc) is a [cyclic edge](#cyclic-edge).
*Feedback arc* is the established name from the literature on directed graphs, which says *arc* for an [edge](#edge).

In graph theory, a *cut* splits the [nodes](#node) into two parts, and the *cut set* is the set of [edges](#edge) between them.
A cut concerns connectivity, and a [feedback arc set](#feedback-arc-set) concerns [cycles](#cycle).

cargo-arc builds the [feedback arc set](#feedback-arc-set) greedily.
It is not the *minimum* feedback arc set, which is NP-hard to find.
Where the difference matters, say *greedy feedback arc set*.
The [report](#report) describes the set in a sentence without naming it ([RULES.md](RULES.md#no-cycles)).

*Traffic* is a term of this project.
*Edge betweenness* is the nearest established term, and it counts the shortest paths through an [edge](#edge) instead of [cycles](#cycle).

Removing a [feedback arc](#feedback-arc) moves code, and Lakos names the two directions it can go.
*Escalation* moves mutually dependent functionality into a component above both modules, an existing one or a new one.
*Demotion* moves common functionality into a component below both.
They are two of the nine levelization techniques in Lakos, *Large-Scale C++* Vol. I, section 3.5.
cargo-arc does not use either word yet; a hint that proposes a move should use them.

## Module roles

| Term | Definition |
|------|------------|
| **<a id="vocabulary">Vocabulary</a>** | The types, constants and errors that a module holds for its descendants to read. A [cycle](#cycle) whose [edges](#edge) toward an ancestor carry only vocabulary is intended and is not debt. |
| **<a id="facade">Facade</a>** | A module that is the entry point to its subtree for all code outside it. It declares and re-exports its children and may call into them, but holds nothing they read back. Its [edges](#edge) point down only, so it closes no [cycle](#cycle) on its own. |
| **<a id="container-module">Container module</a>** | A [facade](#facade) with no code of its own, only `mod` declarations and `pub use`. |
| **<a id="prelude">Prelude</a>** | A module that holds nothing of its own and re-exports names defined elsewhere, so that other code can import them with one glob import. All its [edges](#edge) are [re-exports](#re-export). |

In Rust a parent module is often both the [facade](#facade) of its subtree and the holder of the subtree's [vocabulary](#vocabulary).
A [cycle](#cycle) between such a parent and its children has two halves: the children read the vocabulary upward, and the parent calls downward.
[RULES.md](RULES.md#allow) describes the `allow` entries that permit the upward half.

In this section, *up* and *down* follow the module tree.
For a [facade](#facade), Lakos's levels give the same direction.
For [vocabulary](#vocabulary) they give the opposite one: a module that others depend on sits low in his hierarchy, so a parent that holds its subtree's vocabulary sits below its own children there.

Lakos calls types that pass through function boundaries, such as a date or an allocator, *vocabulary types*, and places them low (Vol. I, section 0.4).
The idea is the same, but his term names a property of a type, and *vocabulary* here names what a module holds.
He has no word for a parent that holds its children's types.

*Facade* has the meaning Lakos and the Gang of Four give it: one interface over a whole subsystem.
Lakos places a [facade](#facade) above what it wraps (Vol. I, section 0.7), and the levelization technique that builds one is *escalating encapsulation*.
Lakos also calls a facade a *wrapper*.
In Rust a wrapper is usually a newtype around a single type.

With a [facade](#facade), a detail behind it can change without its clients changing their code, which Lakos calls *encapsulation*.
His *insulation* also spares the clients a recompile (Vol. I, section 3.11.1).
In Rust the crate is the unit of compilation, so a module facade cannot spare a recompile to a client in the same crate.

A [prelude](#prelude) re-exports names like a [facade](#facade), but the names come from outside its subtree, often from its parent, so its [edges](#edge) point up.
A re-export passes a name on without depending on it, so a prelude depends on nothing.

In C4, a *container* is an application or a data store, a unit that runs on its own.
A workspace with several binaries holds several C4 containers, and no module is one.
Say *container module* for a module and *C4 container* for the deployable unit.
In the [hotspot map](#hotspot-map), a [container](#container) is a circle that holds other circles.

Lakos calls a unit that only groups components a *package*; it is not a component itself.
In Cargo, a *package* is what a `Cargo.toml` describes.
A Lakos component, a header with its implementation file, corresponds to a module file here.

## Rules and violations

| Term | Definition |
|------|------------|
| **<a id="rule">Rule</a>** | One named check in `arc-rules.toml`, of type `layers`, `forbidden-dependency` or `no-cycles`. Its name is unique across all types. |
| **<a id="severity">Severity</a>** | How bad it is to break a [rule](#rule): `error`, `warn` or `ignore`. Severity belongs to the rule, and all its [violations](#violation) share it. |
| **<a id="violation">Violation</a>** | A dependency or a [tangle](#tangle) that breaks a [rule](#rule). The dependency can be direct or transitive. A manifest entry and an import between the same two ends are one dependency. Every violation is in exactly one of the three states below. |
| **<a id="reported">Reported</a>** | The state of a [violation](#violation) that is neither [allowed](#allowed) nor [frozen](#frozen). Only reported violations affect the exit code. |
| **<a id="allowed">Allowed</a>** | Permitted by an `allow` entry on the [rule](#rule), permanently and on purpose. An entry names an [edge](#edge) that runs against the order the rule's writer has in mind, and the entries of a rule together declare that order. |
| **<a id="frozen">Frozen</a>** | Covered by an entry in `arc-baseline.toml`. A frozen [violation](#violation) is debt that is tolerated until someone fixes it and is expected to shrink. |
| **<a id="silenced">Silenced</a>** | [Allowed](#allowed) or [frozen](#frozen). `--show-silenced` lists silenced [violations](#violation). Silenced is not a state of its own. |
| **<a id="baseline">Baseline</a>** | The set of [frozen](#frozen) [violations](#violation), kept in `arc-baseline.toml` beside the rules file. Only `--generate-baseline` writes it. |
| **<a id="diagnostic">Diagnostic</a>** | A gap in the configuration: a [node](#node) that an [exhaustive `layers` rule](#exhaustive-rule) leaves in no position, an `except` entry that matches no node, a [baseline](#baseline) entry that matches nothing, a baseline entry that freezes more symbols than the [edge](#edge) still carries, an `allow` entry that matches nothing, `allow` entries that put nodes above each other in a circle, a [rule](#rule) [pattern](#module-path-pattern) that matches nothing, or a catch-all [layer](#layer) that holds nothing. |
| **<a id="diagnostic-level">Diagnostic level</a>** | Whether a run tolerates the gap a [diagnostic](#diagnostic) names: `allow`, `warn` or `deny`. |
| **<a id="layer">Layer</a>** | One position in a `layers` [rule](#rule). It holds one or more [patterns](#module-path-pattern), or `*` for the [nodes](#node) that no other layer holds. Patterns in the same layer share its position. |
| **<a id="exhaustive-rule">Exhaustive rule</a>** | A `layers` [rule](#rule) with `exhaustive = true`, which claims to sort everything it addresses. Its crate patterns claim every workspace crate, and its module patterns claim every module of the crates they reach. A rule without the field says nothing about the [nodes](#node) it does not name. |
| **<a id="module-path-pattern">Module path pattern</a>** | A module path with optional wildcards, such as `domain`, `domain::service`, `domain::*`, `domain::**`, `domain*` or a bare `**`. Inside one segment, `*` stands for any run of characters. *Pattern* alone means a module path pattern unless the text says otherwise. |
| **<a id="dependency-pattern">Dependency pattern</a>** | A named list of `allow` entries under `[dependency-patterns]`, which a [rule](#rule)'s `allow` list refers to by name. It only selects dependencies; the `allow` list that names it allows them. |
| **<a id="scope">Scope</a>** | The [module path pattern](#module-path-pattern) in the `scope` field of a `no-cycles` [rule](#rule). The rule searches only the [edges](#edge) whose two ends the pattern matches. |

Semgrep and Detekt call a [violation](#violation) a *finding*.

A *whitelist* is a list of exceptions, and the word fits [allowed](#allowed) and [frozen](#frozen) [violations](#violation) alike, so it does not tell them apart.
An *ignored* [rule](#rule) has [severity](#severity) `ignore` and is never checked, while an allowed violation was found and then permitted.
*Baselined* says only that a violation has an entry in `arc-baseline.toml`.
Linters usually call a hidden result *suppressed*, and cargo-arc calls it *silenced*.
In the [arc diagram](#arc-diagram), a suppressed arc is one the diagram does not draw (see [The arc diagram](#the-arc-diagram)).

A [diagnostic](#diagnostic) is not a [violation](#violation) and has no [severity](#severity).

The configuration and the output use different words.
[Severity](#severity) and [diagnostic level](#diagnostic-level) are configured as `error`, `warn`, `ignore`, `allow` and `deny`.
The output calls what a run produced an *error* or a *warning*: a [violation](#violation) of severity `warn` is printed and counted as a warning, and a [diagnostic](#diagnostic) at level `deny` is printed as an error.
A [rule](#rule)'s block with no [reported](#reported) violation is headed `silenced` instead.

A shell *glob* also has character classes, alternation and negation.
A [module path pattern](#module-path-pattern) has none of them.
Its `*` never crosses a `::`, and `**` stands only as the whole pattern or as its last segment.

In the frontend code, a *layer* is also an SVG stacking order.
In the architecture literature, a *tier* is a deployment boundary.

*Total* and *complete* would claim the whole workspace.
An [exhaustive rule](#exhaustive-rule) written only from module patterns covers the modules of the crates it reaches and says nothing about other crates.

### What a run prints

| Term | Definition |
|------|------------|
| **<a id="report">Report</a>** | The blocks that `check` writes to stderr, one per [rule](#rule) that fired. A block is headed by the rule and lists its [violations](#violation), each with its dependency or [tangle](#tangle) and its [locations](#location). |
| **<a id="status">Status</a>** | The outcome of one [rule](#rule) in a run: `ok`, `WARN` when the rule produced only warnings, or `FAILED` when it produced an error. `check` prints one status line on stdout for each checked rule and one for the configuration, whether or not anything fired. |
| **<a id="location">Location</a>** | A file and a line, such as the place of an import that creates an [edge](#edge). The [report](#report) prints a location as `--> path:line`, and the sidebar of the [arc diagram](#arc-diagram) lists it. The path is relative to the root of the analyzed workspace, and a file outside the workspace keeps its absolute path. |

[Severity](#severity) is configured on a [rule](#rule), and a [status](#status) is the result of a run.
The severity does not determine the status: a rule of severity `error` whose [violations](#violation) are all [frozen](#frozen) has status `ok`.

In the code, the *source* of an [edge](#edge) is the [node](#node) it starts from, not a [location](#location).

## Hotspots

| Term | Definition |
|------|------------|
| **<a id="code-lines">Code lines</a>** | Tokei's `code` count for a file: its source lines without comments and blank lines. |
| **<a id="container">Container</a>** | A circle that holds other circles: the workspace, a crate, or a module with children. The file that declares it appears as a [leaf](#leaf) inside it. |
| **<a id="leaf">Leaf</a>** | The circle of a single file. Size and commits are measured on leaves. |
| **<a id="hotspot">Hotspot</a>** | One of the top-N files of the workspace, ranked by [code lines](#code-lines) times commit count. A hotspot is always a [leaf](#leaf). |
| **<a id="hotspot-map">Hotspot map</a>** | The view of a workspace as nested circles, sized by [code lines](#code-lines) and colored by commit activity. |
| **<a id="label-band">Label band</a>** | The strip at the top of a [container](#container) that stays free for the container's label. The container's children are shifted down by its height. |

*LOC* and *lines of code* can include comments and blank lines; [code lines](#code-lines) never do.

*Hot file* names a property that a file has or lacks, and a [hotspot](#hotspot) is a rank among all files.

*Group* and *folder* suggest a directory in the file system, and a [container](#container) can also be a crate or the workspace.

A [node](#node) is a crate or module in the dependency graph, and a [leaf](#leaf) is the circle of one file in the [hotspot map](#hotspot-map).

A *treemap* nests rectangles.
cargo-arc tried one and draws nested circles instead, which show the nesting depth better but leave space unused.

## Symbols and consumers

| Term | Definition |
|------|------------|
| **<a id="provider">Provider</a>** | A module that other modules import symbols from. |
| **<a id="consumer">Consumer</a>** | A module that imports a symbol from a [provider](#provider) in production code. A module that imports a symbol only by `pub use` republishes it and is not its consumer. |
| **<a id="consumer-group">Consumer group</a>** | The symbols of one [provider](#provider) that have exactly the same [consumers](#consumer). |
| **<a id="consumer-locality">Consumer locality</a>** | How close the [consumers](#consumer) of a [consumer group](#consumer-group) sit to each other in the module tree. It has three cases: a single consumer, several consumers under a common ancestor module that does not contain the [provider](#provider), or consumers spread across the crate. |

[Consumer locality](#consumer-locality) shows whether the symbols of a group could move closer to the modules that use them.
[ARC_DIAGRAM.md](ARC_DIAGRAM.md#the-sidebar-for-an-arc) gives the sidebar's wording for each case.

A [consumer group](#consumer-group) is the unit that can move.
Its symbols share one set of [consumers](#consumer), so the group's locality holds for each of them.

In this file, a [cluster](#cluster) is a strongly connected component, and a [scope](#scope) is the [pattern](#module-path-pattern) a `no-cycles` [rule](#rule) searches inside.

## The arc diagram

| Term | Definition |
|------|------------|
| **<a id="arc-diagram">Arc diagram</a>** | The view of a workspace as a tree of crates and modules, with arcs for the dependencies between them. `cargo arc` renders it, and `cargo arc ui` serves it beside the [hotspot map](#hotspot-map). |
| **<a id="arc-type">Arc type</a>** | The category of dependency an arc draws: crate-dep, module-dep or [re-export](#re-export). Every arc has exactly one arc type. cargo-arc derives it from the arc's endpoints and its re-export flag and does not store it. |
| **<a id="re-export">Re-export</a>** | The [arc type](#arc-type) of an arc whose imports are all `pub use`, so that it passes names on without depending on them. A single ordinary import on the arc makes it a module-dep instead ([ADR-022](adr/022-reexport-edges-tagged-not-dropped.md)). |
| **<a id="filter">Filter</a>** | A checkbox in the **View** menu that shows or hides part of the diagram. Four filters cover arcs: crate dependencies, module dependencies, [re-exports](#re-export) and circular dependencies. The others cover [nodes](#node). |
| **<a id="reading-order">Reading order</a>** | The top-to-bottom sequence of the [nodes](#node) under one parent. A node comes before the nodes it depends on. Inside a [tangle](#tangle) no order can do that for every [edge](#edge), and the layout picks the order with the least dependency weight pointing up. A tangle of more than eight nodes under one parent is ordered alphabetically instead. |
| **<a id="upward-edge">Upward edge</a>** | An [edge](#edge) whose target sits above its source in the [reading order](#reading-order). Inside a [tangle](#tangle) some edge must be upward. Outside a tangle, an upward edge occurs only where two subtrees each hold a module that uses a module of the other: a [cycle](#cycle) between the subtrees without a cycle between modules. The diagram draws upward edges in their own style. |

In this section and in [ARC_DIAGRAM.md](ARC_DIAGRAM.md), *diagram* alone means the [arc diagram](#arc-diagram).
Where a text also covers the [hotspot map](#hotspot-map), say *arc diagram*.

*Kind* has two other meanings in this project.
It says whether a reference is in production or in test code, and in Cargo it classifies a manifest dependency as normal, dev or build.
Both are independent of the [arc type](#arc-type): one pair of crates can have a production [edge](#edge) and a test edge of the same arc type.
*Level* would suggest an order among the three arc types, and they have none.

Three of the four arc [filters](#filter) select on the [arc type](#arc-type).
The circular-dependencies filter selects on whether the arc is a [cyclic edge](#cyclic-edge), so an arc can fall under two filters at once.
[ARC_DIAGRAM.md](ARC_DIAGRAM.md#filters) describes which arcs a set of filters leaves visible.

A [layer](#layer) is a position in a `layers` [rule](#rule) or an SVG stacking order.
All arcs sit in one SVG stacking layer, whichever [filters](#filter) cover them.

A *suppressed* arc is one the diagram does not draw: a crate arc between two crates whose modules a module arc already connects, or, in group mode, an arc that does not touch the selected [node](#node).

The checkbox of the circular-dependencies [filter](#filter) also turns on *cluster mode*, which is not a filter.

The [reading order](#reading-order) follows a levelization in Lakos's sense, and Structure101 calls an [edge](#edge) against it a *feedback (upward) dependency*.
A [node](#node)'s *level* is the one Lakos defines: 0 for what lies outside the workspace, 1 for a node that depends on nothing inside it, and otherwise one more than the highest level among the nodes it depends on (Vol. I, section 1.10).
Levels are not positions in the reading order, and several nodes can share a level.
Lakos gives every member of a [cycle](#cycle) the highest level the cycle spans (footnote 128), so levels cannot order the nodes of a [tangle](#tangle).
The *diagnostic level* of a rules file is unrelated and is always written with *diagnostic*.

Whether an [edge](#edge) is a *back edge* depends on where a depth-first search starts (see [Cycles and clusters](#cycles-and-clusters)).
Whether an edge is [upward](#upward-edge) depends only on the [reading order](#reading-order), which a reader can see in the diagram.
In package managers, the *reverse dependencies* of a package are the packages that depend on it.

An [upward edge](#upward-edge) inside a [tangle](#tangle) is a [cyclic edge](#cyclic-edge).
It need not be a [feedback arc](#feedback-arc): the layout weighs module [edges](#edge) between sibling subtrees, the cycle search looks at [cycles](#cycle) between modules, and the two can pick different edges.
Deriving the [reading order](#reading-order) from the [feedback arc set](#feedback-arc-set) is open work.
