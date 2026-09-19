# Architecture rules

<!-- TOC -->

- [Adopting rules](#adopting-rules)
  - [The run before you have a file](#the-run-before-you-have-a-file)
  - [Your first rule](#your-first-rule)
  - [Nodes a rule leaves out](#nodes-a-rule-leaves-out)
  - [Introducing a rule the workspace already breaks](#introducing-a-rule-the-workspace-already-breaks)
- [Reference](#reference)
  - [Where the files live](#where-the-files-live)
  - [The `[config]` table](#the-config-table)
  - [Module path patterns](#module-path-patterns)
  - [`layers`](#layers)
  - [`forbidden-dependency`](#forbidden-dependency)
  - [`no-cycles`](#no-cycles)
    - [How bad a tangle is](#how-bad-a-tangle-is)
  - [Severity](#severity)
  - [`allow`](#allow)
  - [Diagnostics](#diagnostics)
  - [The baseline](#the-baseline)
  - [What a run prints](#what-a-run-prints)
  - [Flags](#flags)
    - [What the feature flags change](#what-the-feature-flags-change)

<!-- /TOC -->
`cargo arc check` evaluates a workspace against the rules you write in `arc-rules.toml`.
Rules sort your crates and modules into layers, forbid circular dependencies between modules, or ban a dependency you name.
`check` reports the violations it finds and exits non-zero on any you have not accepted, so you can run it in CI next to the test suite.

```bash
cargo arc check
```

## Adopting rules

[`arc-rules.example.toml`](arc-rules.example.toml) is the file this section arrives at.

### The run before you have a file

By default `cargo arc` forbids circular dependencies between modules anywhere in the workspace:

```
error[no-cycles]: no cycles
    tangle 1/1: core (2 modules, 1 cycle)
      cycle: model -> ids -> model
      edges:
        ids   -> model (on 1 cycle, 1 symbol)
        model -> ids   (on 1 cycle, 2 symbols)
```

The report goes to stderr, one status line per rule to stdout:

```
no cycles FAILED: 1 errors, 0 warnings, 0 allowed, 0 frozen
config ok: 0 errors, 0 warnings
```

An existing workspace might have circular dependencies, and some of those will not be removed in the foreseeable future.
The baseline freezes the violations that exist today, so a check succeeds and reports only what is added after it.

```bash
cargo arc check --generate-baseline
```

This writes an `arc-baseline.toml` beside the rules file (or, without one, beside `Cargo.toml`).
It only reports how many violations it froze.
Every new cycle is reported.

### Your first rule

Write one rule, run, read what it says, freeze what you accept, then write the next one.
A run that generates a baseline over several rules at once freezes their violations before anyone has decided about any of them, and a baseline nobody has read does not shrink.

A `layers` rule states the order of your crates, one entry in `layers` per position:

```toml
[[rules]]
type = "layers"
name = "architecture layers"
layers = [["cli", "storage"], "services", "core"]
direction = "top-down"
```

`direction = "top-down"` makes the first entry the top layer, so a dependency may only point at a later one.
`cli` and `storage` share a position because neither sits above the other, and the rule then says nothing about dependencies between them.

`cargo arc` works out no order of its own, so the list is yours to write.
Run it:

```
error[layers]: architecture layers
  = core::model → services::registry
    --> core/src/model.rs:8
```

A reported violation means either the code is wrong or the rule is.
Where the code is right, change the rule.
What is left is debt.
Freeze it:

```bash
cargo arc check --generate-baseline
```

Then the next rule.
A `layers` rule permits every dependency that points downward, including the ones you do not want.
Here the layer order allows `storage` to depend on `services`, and it must not:

```toml
[[rules]]
type = "forbidden-dependency"
name = "no services in storage"
from = "storage"
to = "services"
```

The last rule replaces the one you started with by default:

```toml
[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
```

As soon as the file states a `no-cycles` rule of its own, the default rule is gone.
Keeping the name `no cycles` also keeps the baseline entries written under the default rule, because entries are keyed by rule name.

### Nodes a rule leaves out

A `layers` rule judges the nodes its positions name, and nothing else.
A node no position names is not sorted, and a dependency between two such nodes passes unreported.
A dependency that only runs over such a node is checked, under the pair at its ends.

A position may have been meant to catch that node and left it out by accident.
Say so with `exhaustive = true`, and the run names the node instead:

```toml
[[rules]]
type = "layers"
name = "architecture layers"
layers = [["cli", "storage"], "services", "core"]
direction = "top-down"
exhaustive = true
```

```
error: configuration
  unlayered-node: xtask
    in rule "architecture layers", in no layer, so its own place goes unchecked
```

A pattern without `::` names a crate, and a rule carrying one claims every workspace crate.
The rule above sorts `cli`, `storage`, `services` and `core`, so anything else is a gap.
A pattern with `::` names a module, and a rule carrying one claims every module of the crates its module patterns reach, saying nothing about the other crates and nothing about the crate nodes themselves.
A rule that mixes both kinds of pattern makes both claims side by side.

Only the topmost unsorted node of a containment chain is reported.
A crate left out entirely is one gap, not one per module underneath it.
A crate one exhaustive rule places is still reported against a second exhaustive rule that leaves it out.

If the node was forgotten, put it in a layer.
If it is outside the architecture on purpose (build tooling, examples, benchmarks), say so in the diagnostic:

```toml
[diagnostics]
unlayered-node = { level = "deny", except = ["xtask"] }
```

The `allow` list of a rule does not reach the diagnostics, which is why this list exists separately.
Inventing a layer for `xtask` does not work.
It puts the crate in the order, and every dependency between it and a node in another position is then judged against that order.
The names here are patterns like any other, so an excepted node takes the modules below it with it.

### Introducing a rule the workspace already breaks

`severity = "warn"` looks like the way to introduce a rule gently, and it is the wrong tool.
A warning freezes nothing, so there is no recorded amount to shrink.
It does not stop new violations either.
They are printed beside the old ones and the check still succeeds.
`warn` marks a rule the architect wants to advise rather than block.
Using it as an introduction stage gives one word two meanings, and a `WARN` line then no longer says whether the rule is advice or a rule you are still introducing.

The baseline records how much exists, expects it to shrink, and reports anything beyond it as an error.

## Reference

### Where the files live

`cargo arc check` reads `arc-rules.toml` next to the workspace `Cargo.toml`.
`--rules <path>` points it elsewhere, and `arc-baseline.toml` is then looked up beside that file.
A missing rules file is the implicit run described above; a path given with `--rules` that does not exist is an error.
A missing baseline file means nothing is frozen.

Neither file has to sit in the workspace under test.
`--rules /tmp/probe.toml` reads the rules from there, `--generate-baseline` writes `/tmp/arc-baseline.toml` beside them, and the workspace keeps neither file.
Two rules files in one directory share the one `arc-baseline.toml` there, so a copy made to try a rule out reads the entries of the original and reports its own violations as frozen.

`--manifest-path` selects the workspace, not a part of it.
Pointed at a member crate's `Cargo.toml`, it analyses the whole workspace that crate belongs to.
Without `--rules`, both files are looked up beside the manifest the flag names, so pointing at a member crate looks for `arc-rules.toml` in that crate's directory and not in the workspace root.
Most member crates have no rules file there, and the run then falls back to the implicit check over the whole workspace: the rules in the workspace root are not read.
Pass `--rules` to keep them.

### The `[config]` table

```toml
[config]
version = 1
default_severity = "error"
```

`version` is the file format version and is `1`; a different number halts the run when the file loads.
`default_severity` applies to every rule that does not carry a `severity` of its own; without the section it is `error`.

Unknown keys are rejected rather than ignored, so a typo in a rule or a diagnostic name fails the run instead of switching something off silently.

### Module path patterns

A module path pattern names crates and modules by their path:

| Pattern | Matches |
|---------|---------|
| `core` | the crate `core`, and every module in it |
| `core::model` | the module `core::model`, and every module in it |
| `core::*` | the direct children of `core` |
| `core::**` | every module below `core`, not `core` itself |
| `core*` | every crate starting with `core`, and every module in each |
| `**` | everything in the workspace |

There is no pattern for a crate on its own.
`core` takes its modules with it, and `core::**` leaves the crate out.
An `allow` entry with a crate name on both sides therefore covers every dependency between the modules inside it.
Inside one segment, `*` stands for any run of characters, including none.
It may sit anywhere in the segment and appear more than once, but never crosses a `::`.
So `core*` names every crate that starts with `core`, and `*_test` every one that ends with `_test`.
Any segment of a path may hold one, so `core::*::error` matches `core::model::error` and `core::api::error`.
A pattern always starts at a crate name; there is no `crate::` prefix, because a rules file applies to the workspace and not from inside one crate.
That first segment is the package name as `Cargo.toml` writes it, so a crate named `data-store` is `data-store` in a pattern and not `data_store`.
Outside a `layers` position, `*` is an ordinary pattern and matches every crate together with its modules.
A `layers` position may hold `*` only alone, and it is then the catch-all layer, described under [`layers`](#layers).

A pattern reaches the crates under analysis and the modules in them, and nothing else.
A crate the workspace depends on lies outside that reach, so `to = "tokio"` matches nothing.

A crate has the dependencies its `Cargo.toml` declares, and a module has the imports written in its own file.
`storage` covers those, and `storage::**` does not, because it leaves out `storage` itself.
Both cover the modules of `storage`.
`storage*` and `storage*::**` divide the same way.

A pattern that matches nothing fails the run by default (`unmatched-pattern`, see [Diagnostics](#diagnostics)).
A rule built from it checks nothing and would otherwise leave the workspace green.

### `layers`

```toml
[[rules]]
type = "layers"
name = "architecture layers"
layers = [["cli", "storage"], "services", "core"]
direction = "top-down"
```

Every entry in `layers` is one position, holding either a pattern or a list of patterns that share it.
`direction` says which end of the list is the bottom:

- `top-down`: the list starts at the top layer, dependencies point at later entries.
- `bottom-up`: the list starts at the bottom layer, dependencies point at earlier entries.

`exhaustive` is optional and defaults to `false`; setting it makes the rule claim to sort everything it addresses, described under [Nodes a rule leaves out](#nodes-a-rule-leaves-out).

Two nodes sharing a position are unordered, so a dependency between them passes.

What the rule judges against the order is the dependency, whether it is written as one edge or runs over other nodes.
A node no layer matches does not break the rule.
If `services` reaches `core` through an unlayered `util`, the rule reports the dependency `services → core` and lists the edges it runs through underneath it.

```
error[layers]: architecture layers
  = services → core
    services → util
      --> services/src/lib.rs:3
    util → core
      --> util/src/lib.rs:7
```

An `allow` entry or a baseline entry names the pair `services → core`, and that covers the dependency however it runs, including when it later runs over a different node.
The walk stops at every layered node, because the order judges that pair on its own.
Such an entry names two layered nodes, so a silenced edge is never a step of a longer route.
A dependency with an endpoint no layer matches is still not checked, and neither is the place of that node itself; without `exhaustive = true` nothing says which nodes those are.

A position written as the bare string `"*"`, or the single-element list `["*"]`, is the catch-all layer.
It holds every node the rule's other positions do not match, so once a rule carries one, no crate is left unlayered by it.
Its place in the list is its rank like any other position.
A rule may carry at most one catch-all, and it must stand alone in its position.
Both are refused when the rules file loads.
A catch-all whose rest is empty, because the rule's other positions already cover the whole workspace, is reported as `unmatched-pattern`, the same as a pattern matching nothing.
A rule carrying a catch-all may not also be `exhaustive`, and the file is refused when it loads.
The catch-all holds every node the other positions leave, so the claim would check nothing.

A `layers` rule orders positions against each other and needs at least two; with fewer, it is refused when the file loads.
Each position needs at least one pattern to hold; an empty one is refused too.

Two ordinary positions of one rule matching the same node fail the run, naming the node and both positions.

The shortest useful `layers` rule states a single boundary without sorting the rest of the workspace first:

```toml
[[rules]]
type = "layers"
name = "core stays at the bottom"
layers = ["*", "core"]
direction = "top-down"
```

This forbids `core` from depending on anything outside itself.
A dependency the other way, or between any two crates that share the catch-all, is permitted, because `layers` only judges across positions, never within one.

### `forbidden-dependency`

Say `storage::pool` imports from `services::worker`:

```
storage::pool ──► services::worker
```

This rule stays quiet:

```toml
[[rules]]
type = "forbidden-dependency"
name = "no services in the cache"
from = "storage::cache"
to = "services"
```

`storage::cache` covers that module and the modules below it, and `storage::pool` is neither.

This rule reports the dependency:

```toml
[[rules]]
type = "forbidden-dependency"
name = "no services in storage"
from = "storage"
to = "services"
```

A crate name covers every module in the crate, so `storage::pool` matches `from` while `services::worker` matches `to`.

A node matching `from` that depends directly on a node matching `to` is a violation, whether or not a `layers` rule would allow it.
Every import and manifest entry between the same two nodes belongs to that one violation.
A dependency that reaches `services` over a module in between is not reported, and `layers` is the only rule that follows a route like that.

`from` and `to` hold one pattern each, and a node may match both.
`from = "**"` matches every node in the workspace, the crate named in `to` and its modules included.
Its own modules match `from`, so the rule also reports dependencies from one module of that crate to another.

Like every rule, it judges production dependencies only.
An import written under `#[cfg(test)]` or in a build script is not one, and neither is a dev-dependency.

### `no-cycles`

```toml
[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
```

`scope` is the pattern the search runs inside.
An edge takes part when the pattern matches both of its ends.
A cycle running through a module the pattern leaves out is therefore never found.
`scope` holds one pattern and a pattern has no negation, so a scope over the whole workspace except one crate cannot be written.
An [`allow`](#allow) entry naming that crate on both sides does that job instead, because it takes every dependency inside the crate out before the search.

Violations are reported per tangle.
A tangle holding one cycle prints a `cycle:` line naming its modules in order, then one row per edge on it.
Removing any one of those edges breaks the cycle.

A tangle holding more than one cycle prints no `cycle:` line.
It prints its feedback arcs, the edges to remove together, one row each:

```
error[no-cycles]: no cycles
    tangle 1/1: core (5 modules, 3 cycles)
      edges, most cycles first:
        model -> ids  (on 2 cycles, 1 symbol)
        model -> text (on 1 cycle, 1 symbol)
      every circular dependency contains at least one of these 2 edges
```

Where no cycle in the tangle is frozen, removing every edge in the list leaves no circular dependency at all, counted or not.
The closing sentence reads `every circular dependency contains this edge` for a list of one.
It is left out where every listed edge lies on exactly one cycle and the list is as long as the cycle count, because the rows already say it; the heading then reads `edges:` instead.
The cycle count on a row is every counted cycle running through that edge, not only the one the edge itself contributed to the count.

A tangle whose edges are all frozen keeps its whole block, marked `(frozen)` on the tangle line (`tangle 1/1 (frozen): ...`); it only shows up under `--show-silenced`.

A tangle mixing a frozen cycle with counted ones lists only the arcs that break the counted cycles.
The frozen cycle's edges stay in the graph, so the block closes with `an unlisted cycle running through these edges can still stand once every listed edge is gone` in place of the closing sentence above.
Remove every listed edge and run again to see whether an unlisted cycle is left.

The search reads module edges only, so a dependency declared in a `Cargo.toml` is never part of a reported tangle.
A dependency whose imports are all `pub use` counts only under `--include-reexports`: republishing a name is not a dependency on it, and the idiomatic re-export cycles that arise from it are not violations.

A module that uses names of the module containing it closes a cycle with the dependency back down, and the rule reports that cycle like any other.
An [`allow`](#allow) entry whose `to` is `super` removes every edge from a module to the module containing it before the search:

```toml
[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
allow = [
  { from = "**", to = "super", reason = "children read the parent's vocabulary" },
]
```

The crate root contains its top-level modules, so an edge from such a module to its own crate is removed too, where `scope` takes the crate in at all.
`super::super` reaches the module two levels up instead, and `crate` the root of the crate from anywhere inside it.
A removed edge that lay on a cycle is counted as *allowed* and listed under `--show-silenced`; one that lay on none raises no count.
The dependency down from the containing module stays in the search, and so does a cycle between two modules under the same parent.

A file without a `no-cycles` rule is checked by an implicit one named `no cycles` with scope `**`.
Because that rule would still forbid the cycles a narrower rule of yours deliberately permits, writing any `no-cycles` rule removes it.
While the implicit rule is in play its name is reserved: a rule of another type may not be called `no cycles`.

#### How bad a tangle is

A tangle is reported with three numbers, and none of them follows from the other two:

| Number | Question |
|--------|----------|
| modules | how much code is stuck in it |
| cycles | how many circular dependencies run inside it |
| edges in the list | how many places have to change |

None of the three is a cost estimate, and the arc count least of all.
The edges it counts are not equal.
Dropping a re-export that only forwards is close to free; inverting a dependency is not.
The count says how many places have to be touched, not how much work that is.
With a frozen cycle in the mix, the cycle count covers only the counted ones.

### Severity

`severity` on a rule is `error` (the default), `warn` or `ignore`.

A reported violation of an `error` rule fails the run; under `warn` it is reported and counted but the run stays green.
`ignore` switches the rule off entirely: it is never checked, gets no status line, and its baseline entries are left alone.

### `allow`

An `allow` entry permanently allows dependencies under one rule:

```toml
[[rules]]
type = "forbidden-dependency"
name = "no services in storage"
from = "storage"
to = "services"
allow = [
  { from = "storage::migrations", to = "services::schema", reason = "migrations follow the schema" },
]
```

Both sides are patterns, so the entry allows every dependency from one side to the other.
Here that is any module in `storage::migrations` reaching any module in `services::schema`.
An entry allows the one direction it names, and the dependency back needs its own entry.
The two sides may hold the same pattern, and the entry then allows every dependency between the modules it matches, in either direction.
With a crate name on both sides that is every dependency inside the crate, and under `no-cycles` no cycle within it is left to find.
An entry wider than the dependency you meant to allow is never reported.
`unmatched-allow` fires when one of the two patterns matches no module, and says nothing about how much an entry that does match allows.
An entry may also name modules the rule never reaches, outside a `no-cycles` rule's `scope` or outside the `from` and `to` of a `forbidden-dependency`.
It allows nothing there and is not reported, because the diagnostic asks whether the pattern matches a module in the workspace, not whether the rule ever asks about that module.
`reason` is documentation and is never evaluated.
Entries belong to the rule they are written on; there is no shared list.

Under `no-cycles` an allowed dependency is removed before the search, so a cycle running through it never forms in the first place.
Under the other rule types the violation is found and then allowed.
Both count as *allowed* rather than *frozen*, because an entry is meant to stay and a baseline entry is meant to go.
Under `no-cycles` only an allowed edge that lay inside a tangle is counted, and one removed elsewhere raises no count at all.

The entry takes the same form on a `no-cycles` rule:

```toml
[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
allow = [
  { from = "core::keywords", to = "core::writer", reason = "the table is generated from the writer" },
]
```

Allowing either edge of a mutual pair ends the cycle between the two modules, so one entry is enough, and the one you write is the dependency you mean to keep.
Writing both directions takes both edges out of the search, and the status line then counts two allowed instead of one.
Deleting either entry leaves the pair acyclic all the same.

An entry names the odd edge: the one that runs against the order you have in mind, where `to` may depend on `from` by default and the edge back up is the one you tolerate.
The entries of a rule together declare that order, and `contradictory-allow` fires when they put nodes above each other in a circle: one entry `core::a → core::b` beside another `core::b → core::a`, or `to = "super"` beside `to = "self::*"`.
An entry with the same pattern on both sides ranks nothing, and neither does a pair one entry covers in both directions, which is what a crate name in `to` does for the modules inside it.

`to` may be relative to the module `from` matched, so one entry covers the same shape everywhere in the tree:

| `to` | Reaches |
|------|---------|
| `super` | the module containing `from`, or the crate for a top-level module |
| `super::super` | the module two levels up; every further `::super` one more |
| `crate` | the root of the crate `from` lies in |
| `self::*` | the direct children of `from` |
| `self::**` | every module under `from` |

A relative `to` that leads nowhere, `super` from the crate node or `self::*` from a leaf module, matches nothing and is not reported.
`unmatched-allow` asks about the `from` pattern of such an entry only.
Other forms starting with `self`, `super` or `crate` are refused when the file is loaded.

Entries several rules share form a dependency pattern: a named list under `[dependency-patterns]`, referenced by name:

```toml
[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
allow = [
  { pattern = "vocabulary-parent" },
  { from = "core::keywords", to = "core::writer", reason = "the table is generated from the writer" },
]

[dependency-patterns]
vocabulary-parent = [
  { from = "**", to = "super", reason = "children read the parent's vocabulary" },
]
```

A dependency pattern holds entries only and cannot refer to another one.
A reference to a name the file does not define is refused when the file is loaded.
`--show-silenced` marks a violation allowed by such an entry `(allowed by pattern vocabulary-parent)`, and one allowed by an entry written on the rule `(allowed by entry)`.
An edge two entries cover is counted once, under the entry listed first.

`--generate-baseline` refuses to write while an `unmatched-allow` stands, even at level `allow`, because it would freeze the very violations that entry is supposed to allow.

The key `except` of earlier versions is refused with a message naming the entry that replaces it.

### Diagnostics

A diagnostic is a gap in the configuration rather than in the architecture.
Each has a level: `allow` says nothing, `warn` reports without failing, `deny` fails the run.

```toml
[diagnostics]
unlayered-node = { level = "deny", except = ["xtask"] }
unmatched-baseline-entry = "warn"
unmatched-allow = "warn"
unmatched-pattern = "deny"
contradictory-allow = "deny"
```

| Diagnostic | Default | Raised when |
|------------|---------|-------------|
| `unlayered-node` | `deny` | a node an `exhaustive` `layers` rule leaves in no position |
| `unmatched-baseline-entry` | `warn` | a frozen violation the run no longer produces, or one that froze more symbols than the edge still carries |
| `unmatched-allow` | `warn` | an `allow` pattern matching no module |
| `unmatched-pattern` | `deny` | a rule pattern matching no module, or a catch-all layer whose rest is empty |
| `contradictory-allow` | `deny` | the `allow` entries of a rule put nodes above each other in a circle |

`unlayered-node` and `unmatched-pattern` deny where the others warn because both failures leave the run green: a rule whose pattern misses checks nothing, and an unsorted node is never asked where it belongs.
`unlayered-node` also fires only for a rule that carries `exhaustive = true`.
A dead `allow` entry only allows too much, and the violation it should have allowed shows up on its own.
`contradictory-allow` denies because entries that rank a pair both ways say nothing about it, and the rule would tolerate the cycle between the two in silence.

Only `unlayered-node` takes the table form with `except`; the others are written as a level alone.
The names listed there are patterns like any other, so each takes the modules below it with it and a wildcard in one reaches the same nodes it would in a rule.

Diagnostics do not belong to any single rule and are counted on their own status line:

```
config FAILED: 1 errors, 1 warnings
```

### The baseline

`arc-baseline.toml` is written only by `--generate-baseline`, which rewrites every entry from the current violations, except those under a rule at `severity = "ignore"`, which it carries over as they are:

```toml
[config]
version = 1

[[violations]]
rule = "architecture layers"
from = "services::worker"
to = "storage::pool"
symbols = ["Pool"]
```

`version` is the file format version and is `1`; a different number halts the run when the file loads.
An entry freezes one dependency edge under one rule, plus the symbols observed crossing it.
Rule names are unique across all types, because an entry names its rule and not its type.
`bare = true` marks an edge that also carries an import naming no symbol.
A cycle has no entry of its own: it is frozen when every one of its edges is.
A tangle frozen this way is still reported as a tangle: freezing every one of its edges silences its block, it does not remove it.

An entry tolerates only the symbols it names, so a frozen edge does not permit whatever crosses it later.
Once the edge carries a symbol the entry does not name, it is reported again:

```
  = core::writer → core::keywords
    frozen for RESERVED, TYPES; now also carries MODIFIERS
```

Renaming a frozen symbol makes the run report the edge again, and the fix is to regenerate the baseline.
An edge that carries fewer symbols than its entry names is reported as an `unmatched-baseline-entry`, so the file can be narrowed as the debt goes down.

Moving an edge from the baseline into an `allow` entry means deleting its baseline entry.
Left standing, the entry freezes the same edge as before, so removing the `allow` entry again leaves the run green: the edge falls back to *frozen* instead of being reported.
The entry confirms nothing any more and comes back as an `unmatched-baseline-entry`.
That diagnostic is `warn`, so the run stays green and the line sits among the other warnings.
Those warnings are the cleanup list, one decision each: allowed, or fixed in the code.
Regenerating then drops the entry, and under `no-cycles` it can drop more entries than the edges you allowed, because an allowed edge is gone before the search, so a node it held in a tangle leaves with it and that node's own entries are no longer confirmed either.
With the `allow` entry as the only change, regenerating never adds an entry, because it writes one per violation the run still finds and an `allow` entry only takes violations away.

Regenerating over a rules file whose rules were renamed drops every entry that named the old rule.
Entries under a rule at `severity = "ignore"` stay as they are until the rule is checked again; from then on, an ordinary run reports each one it no longer confirms as `unmatched-baseline-entry`.

### What a run prints

stdout carries the status lines, stderr carries the report.
One status line per rule and one for the configuration, printed whether or not anything fired:

```
architecture layers FAILED: 3 errors, 0 warnings, 0 allowed, 0 frozen
no services in storage ok: 0 errors, 0 warnings, 0 allowed, 2 frozen
no cycles WARN: 0 errors, 1 warnings, 0 allowed, 0 frozen
config ok: 0 errors, 0 warnings
```

`ok`, `WARN` and `FAILED` are the three outcomes.
A rule of severity `error` whose violations are all frozen is `ok`.

The report on stderr is one block per rule that fired, headed once by the rule type and name, with the violations underneath:

```
error[forbidden-dependency]: no services in storage
  = storage::pool → services::worker
    --> storage/src/pool.rs:12
    --> storage/src/pool.rs:31
```

A pair appears once in a rule's block: where `Cargo.toml` and an import in the crate's root file write the same dependency, that is one violation, one line, counted once, with the import's locations underneath.

A manifest edge has no line of source behind it, so it is reported without a `-->` underneath and freezes without symbols:

```
  = storage → services
```

An import naming no symbol is written as *an unnamed reference*, so an edge carrying one is never mistaken for an edge carrying nothing.

Without `--show-silenced` the report gives the number of silenced violations instead of listing them:

```
6 violations frozen in the baseline, not counted
  arc check --show-silenced lists them
```

`--show-silenced` folds them into the block of the rule they belong to, marked at the end of their own line; a reported entry next to them carries no mark:

```
error[forbidden-dependency]: no services in storage
  = storage::cache → services::worker
    --> storage/src/cache.rs:8
  = storage::pool → services::worker (frozen)
    --> storage/src/pool.rs:12
    --> storage/src/pool.rs:31
```

An allowed violation names what allowed it: `(allowed by entry)` for an [`allow`](#allow) entry written on the rule, or `(allowed by pattern <name>)` for one that came in through a dependency pattern.

Under the flag, a rule with nothing reported is headed `silenced` instead of `error` or `warning`.

`check` exits with one of three codes:

| Code | Meaning |
|------|---------|
| 0 | no error reported, warnings included |
| 1 | a rule reported an error, or a diagnostic was denied |
| 2 | the run reached no judgment (bad rules file, analysis failure) |

An exit code of 0 covers the modules the analysis walked, and [the README](../README.md#what-the-run-does-not-see) names what it does not reach.

A denied diagnostic is a judgment and exits 1, although its block is headed `configuration` and its status line reads `config FAILED`.
Code 2 is for the run that never got that far: a rules file that does not parse, a `--rules` path that does not exist, a `cargo metadata` that fails.
`--generate-baseline` exits 0 whatever it froze.

The exit code does not say which rule failed.
For separate red builds per concern, use two CI steps with a rules file each.

### Flags

| Flag | Effect |
|------|--------|
| `--rules <path>` | rules file to use, and where the baseline is looked up |
| `--generate-baseline` | rewrite `arc-baseline.toml` instead of checking |
| `--show-silenced` | list the allowed and frozen violations instead of counting them |

The subcommand has no others.
The flags `check` shares with the diagram are written before it: `--manifest-path`, `--features`, `--all-features`, `--no-default-features`, `--include-tests`, `--include-reexports` and `--debug`.

```bash
cargo arc --manifest-path crates/Cargo.toml --include-reexports check
```

The rest of what `cargo arc --help` lists belongs to the diagram.
`check` accepts them and ignores them.
`--externals` before `check` changes nothing, because no rule pattern ever matches a crate outside the workspace.

#### What the feature flags change

`--features`, `--all-features` and `--no-default-features` decide which optional dependencies cargo resolves.
An optional dependency whose feature is off is not resolved, and no rule sees it.

They do not decide which source is read.
An import counts no matter which `#[cfg(feature = "…")]` stands over it, so `--no-default-features` leaves the imports of a default feature in the graph.
A workspace whose features exclude one another carries the imports of all of them at once.

`--features` does one thing more.
Only the workspace members that declare one of the named features are analysed, together with the members those reach through their dependencies.
A rule whose pattern names one of the crates left out reports `unmatched-pattern`.
A feature name no crate in the workspace declares ends the run at exit 2, refused by `cargo metadata`.
`--all-features` and `--no-default-features` leave the set of analysed crates alone.

