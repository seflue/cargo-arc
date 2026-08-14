# Architecture rules

`cargo arc check` holds a workspace against rules you write down in `arc-rules.toml`: which crate may depend on which, where circular dependencies are unacceptable, which single dependency must never appear.
It reports what it finds and exits non-zero on a violation, so it belongs in CI next to the test suite.

```bash
cargo arc check
```

Terms used here are defined in [GLOSSARY.md](GLOSSARY.md).
The rules file this document quotes from is [`arc-rules.example.toml`](arc-rules.example.toml), complete and ready to copy once the crate names in it are yours.

## The run before you have a file

A workspace without an `arc-rules.toml` is not unchecked.
`cargo arc check` falls back to a single implicit rule named `no cycles` that forbids circular dependencies anywhere:

```
error[no-cycles]: no cycles
    tangle 1/1: core (2 modules, 1 cycle)
      cycle: model -> ids -> model
      fewest symbols: ids -> model (1 symbol)
```

The report goes to stderr, one status line per rule to stdout:

```
no cycles FAILED: 1 errors, 0 warnings, 0 allowed, 0 frozen
config ok: 0 errors, 0 warnings
```

A workspace that has grown for years is unlikely to come out clean, and that is what the baseline is for: it freezes what exists today so the run turns green, and reports everything added after it.

```bash
cargo arc check --generate-baseline
```

This writes `arc-baseline.toml` beside the rules file (or, without one, beside `Cargo.toml`) and judges nothing.
Commit it, put `cargo arc check` in CI, and every new cycle is a red build.
Nothing here needs a rules file.

## Your first own rule

Write one rule, run, read what it says, freeze what you accept, then write the next one.
The point of the round is the reading: a run that generates a baseline over five rules at once freezes a pile nobody has looked at, and a pile nobody has looked at does not shrink.

Start with the shape of the workspace.
A `layers` rule states the order of your crates and gets one line per position:

```toml
[[rules]]
type = "layers"
name = "architecture layers"
layers = [["cli", "storage"], "services", "core"]
direction = "top-down"
```

The list runs from the top layer to the bottom one, so a dependency may only point at a later entry.
`cli` and `storage` share a position because neither sits above the other, and the rule then says nothing about dependencies between them.

That order is your assertion about the architecture, and it is the part no generator can supply.
Run it and read what comes back:

```
error[layers]: architecture layers
  = core::model → services::registry
    --> core/src/model.rs:8
```

Two things can be true of a finding: it is a mistake in the code, or it is a mistake in the rule.
Fixing the rule is the honest answer whenever the code is right, and it happens most often in the first round.
What is left is debt.
Freeze it:

```bash
cargo arc check --generate-baseline
```

Then the next rule.
`layers` alone does not finish the job: a dependency that points downward passes it, however wrong it is.
Here `storage` is allowed to reach into `services` by layer order, and must not:

```toml
[[rules]]
type = "forbidden-dependency"
name = "no services in storage"
from = "storage"
to = "services"
```

A rules file that stops after `layers` is the file an architect tends to consider complete, and it is not.

The last rule replaces the implicit one you started with:

```toml
[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
```

As soon as the file states a `no-cycles` rule of its own, the implicit rule is gone, including the wide scope it carried.
Keeping the name `no cycles` also keeps the baseline entries written under the implicit rule: they are keyed by rule name.

## The warning you did not ask for

From the first `layers` rule onwards, a run reports every crate that no layer matches:

```
warning: configuration
  unlayered-crate: xtask
    in no layer, so its edges go unchecked
```

`layers` is a total statement about the workspace, so a crate missing from it is not permitted, it is unsorted: no rule looks at its dependencies at all.
This is the only feedback on whether your file describes the whole workspace, and there are two very different reasons for a crate to show up in it.

If the crate was forgotten, put it in a layer.
If it is outside the architecture on purpose (build tooling, examples, benchmarks), say so in the diagnostic:

```toml
[diagnostics]
unlayered-crate = { level = "warn", except = ["xtask"] }
```

The `except` on a rule does not reach the diagnostics, which is why this list exists separately.
What does not work is inventing a layer for `xtask`: that sorts it into an order it has no place in, and the rule then asserts something about it.

## Debt goes in the baseline, not on `warn`

`severity = "warn"` looks like the way to introduce a rule gently, and it is the wrong tool.
A warning freezes nothing, so nothing can shrink; it does not stop new violations either, it just collects them quietly.
And `warn` already means something: a rule the architect wants to advise rather than block.
Using it as an introduction stage gives one word two meanings, and afterwards no status line can tell an intended `WARN` from an unfinished rollout.

The baseline says the opposite of a warning: this much exists, it is expected to shrink, and anything beyond it is red today.

## Reference

### Where the files live

`cargo arc check` reads `arc-rules.toml` next to the workspace `Cargo.toml`.
`--rules <path>` points it elsewhere, and `arc-baseline.toml` is then looked up beside that file.
A missing rules file is the implicit run described above; a path given with `--rules` that does not exist is an error.
A missing baseline file means nothing is frozen.

```toml
[config]
version = 1
default_severity = "error"
```

`version` is the file format version and is `1`.
`default_severity` applies to every rule that does not carry a `severity` of its own; without the section it is `error`.

Unknown keys are rejected rather than ignored, so a typo in a rule or a diagnostic name fails the run instead of switching something off silently.

### Patterns

A pattern names crates and modules by their path:

| Pattern | Matches |
|---------|---------|
| `core` | the crate `core`, and every module in it |
| `core::model` | that one module |
| `core::*` | the direct children of `core` |
| `core::**` | every module below `core`, not `core` itself |
| `**` | everything in the workspace |

Wildcards cut at `::` only, so there is no `core_*`.
A pattern always starts at a crate name; there is no `crate::` prefix, because a rules file applies to the workspace and not from inside one crate.

The difference between `storage` and `storage::**` matters for crate-level dependencies: only the first covers the crate node itself, so only it sees the dependency declared in `Cargo.toml`.
Both cover the modules.

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

Two nodes sharing a position are unordered, so a dependency between them passes.
A dependency with an endpoint that no layer matches is not checked at all; the crates this happens to are reported as `unlayered-crate`.

### `forbidden-dependency`

```toml
[[rules]]
type = "forbidden-dependency"
name = "no services in storage"
from = "storage"
to = "services"
```

Every dependency from a node matching `from` to a node matching `to` is a violation, whether or not any layer rule would allow it.

Like every rule, it judges production dependencies only.
An import written under `#[cfg(test)]` or in a build script is not one, and neither is a dev-dependency.

### `no-cycles`

```toml
[[rules]]
type = "no-cycles"
name = "no cycles"
scope = "**"
```

`scope` is the pattern the search runs inside; only dependencies between two nodes in scope take part.
Violations are reported per tangle.
A tangle holding exactly one cycle is written out in full, and the edge carrying the fewest symbols is named below it.
A tangle holding several gets the ranked feedback arcs, stated in prose as "every cycle contains at least one of these edges".
An edge appears with every reported cycle it lies on, not only with the shortest one it stands in for.

Crate-level dependencies never take part in the search: a cycle between crates is a thing Cargo already refuses, and what is left runs between modules.
A dependency whose imports are all `pub use` counts only under `--include-reexports`: republishing a name is not a dependency on it, and the idiomatic re-export cycles that arise from it are not violations.

A file without a `no-cycles` rule is checked by an implicit one named `no cycles` with scope `**`.
Because that rule would still forbid the cycles a narrower rule of yours deliberately permits, writing any `no-cycles` rule removes it.
While the implicit rule is in play its name is reserved: a rule of another type may not be called `no cycles`.

#### How bad a tangle is

The numbers a tangle is reported with run along three axes, and no single one of them carries the other two:

| Axis | Number | Question |
|------|--------|----------|
| Extent | modules | how much code is stuck in it |
| Intensity | cycles | how tightly it is woven |
| Feedback arcs | set size | how many edges have to go |

None of the three is a cost estimate, and the arc count least of all.
Greedy cover makes it an upper bound rather than the minimum, and the edges are not equal.
Dropping a re-export that only forwards is close to free; inverting a dependency is not.
It says how many places have to be touched, not how much work that is.

### Severity

`severity` on a rule is `error` (the default), `warn` or `ignore`.

A reported violation of an `error` rule fails the run; under `warn` it is reported and counted but the run stays green.
`ignore` switches the rule off entirely: it is never checked, gets no status line, and its baseline entries are left alone.

Rule names are unique across all types, because a baseline entry names its rule and not its type.

### `except`

An `except` entry permanently allows one dependency under one rule:

```toml
[[rules]]
type = "forbidden-dependency"
name = "no services in storage"
from = "storage"
to = "services"
except = [
  { from = "storage::migrations", to = "services::schema", reason = "migrations follow the schema" },
]
```

Both sides are patterns.
`reason` is documentation and is never evaluated.
Exceptions belong to the rule they are written on; there is no shared list.

Under `no-cycles` an excepted dependency is removed before the search, so a cycle running through it never forms in the first place.
Under the other rule types the violation is found and then allowed.
Either way it counts as *allowed*, not as *frozen*: an exception is meant to stay, a baseline entry is meant to go.

An `except` whose pattern matches no module is reported (`unmatched-except`), and `--generate-baseline` refuses to write while one exists: it would freeze the very violations that entry is supposed to allow.

### Diagnostics

A diagnostic is a gap in the configuration rather than in the architecture.
Each has a level: `allow` says nothing, `warn` reports without failing, `deny` fails the run.

```toml
[diagnostics]
unlayered-crate = { level = "warn", except = ["xtask"] }
unmatched-baseline-entry = "warn"
unmatched-except = "warn"
unmatched-pattern = "deny"
```

| Diagnostic | Default | Raised when |
|------------|---------|-------------|
| `unlayered-crate` | `warn` | a crate no `layers` rule sorts into a position |
| `unmatched-baseline-entry` | `warn` | a frozen violation the run no longer produces, or one that froze more symbols than the edge still carries |
| `unmatched-except` | `warn` | an `except` pattern matching no module |
| `unmatched-pattern` | `deny` | a rule pattern matching no module |

`unmatched-pattern` denies where the others warn because of what its failure looks like: a rule whose pattern misses checks nothing, reports nothing, and leaves the run green.
A dead `except` only allows too much, and the violation it should have allowed shows up on its own.

Only `unlayered-crate` takes the table form with `except`; the others are written as a level alone.
The crates listed there are the ones deliberately outside the architecture.

Diagnostics do not belong to any single rule and are counted on their own status line:

```
config FAILED: 1 errors, 1 warnings
```

### The baseline

`arc-baseline.toml` is written only by `--generate-baseline`, which rewrites it from scratch from the current violations:

```toml
[config]
version = 1

[[violations]]
rule = "architecture layers"
from = "services::worker"
to = "storage::pool"
symbols = ["Pool"]
```

An entry freezes one dependency edge under one rule, plus the symbols observed crossing it.
`bare = true` marks an edge that also carries a reference the resolver could not name.
A cycle has no entry of its own: it is frozen when every one of its edges is.

Freezing the symbols is what keeps a frozen edge from becoming a licence.
Once the edge carries something new, it is reported again:

```
  = core::writer → core::keywords
    frozen for RESERVED, TYPES; now also carries MODIFIERS
```

The price is that renaming a frozen symbol turns the entry red, and the answer is to regenerate.
Shrinking is reported the other way round, as an `unmatched-baseline-entry`, so the file can be narrowed as the debt goes down.

Regenerating over a rules file whose rules were renamed drops every entry that named the old rule.

### What a run prints

stdout carries the judgment, stderr carries the report.
One status line per rule and one for the configuration, printed whether or not anything fired:

```
architecture layers FAILED: 3 errors, 0 warnings, 0 allowed, 0 frozen
no services in storage ok: 0 errors, 0 warnings, 0 allowed, 2 frozen
no cycles WARN: 0 errors, 1 warnings, 0 allowed, 0 frozen
config ok: 0 errors, 0 warnings
```

`ok`, `WARN` and `FAILED` are the three outcomes.
A rule of severity `error` whose violations are all frozen is `ok`: the status says how the run came out, not how the rule is configured.
`allowed` and `frozen` stay apart because one is meant to stay and the other to shrink.

The report on stderr is one block per rule that fired, headed once by the rule type and name, with the violations underneath:

```
error[forbidden-dependency]: no services in storage
  = storage::pool → services::worker
    --> storage/src/pool.rs:12
    --> storage/src/pool.rs:31
```

Silenced violations are counted rather than listed, and `--show-silenced` lists them under an `except[...]` or `baseline[...]` header:

```
6 violations frozen in the baseline, not counted
  arc check --show-silenced lists them
```

Exit codes are flat:

| Code | Meaning |
|------|---------|
| 0 | nothing reported |
| 1 | a rule reported an error, or a diagnostic was denied |
| 2 | the run reached no judgment (bad rules file, analysis failure) |

If you want separate red builds for separate concerns, use two CI steps with a rules file each.
There is no bitset.

### Flags

| Flag | Effect |
|------|--------|
| `--rules <path>` | rules file to use, and where the baseline is looked up |
| `--generate-baseline` | rewrite `arc-baseline.toml` from the current violations instead of checking |
| `--show-silenced` | list the allowed and frozen violations instead of counting them |

The flags that shape the analysis are shared with the diagram and are written before the subcommand: `--manifest-path`, `--features`, `--all-features`, `--no-default-features`, `--include-tests`, `--include-reexports` and `--debug`.

```bash
cargo arc --manifest-path crates/Cargo.toml --include-reexports check
```

## In CI

```yaml
- name: Architecture rules
  run: cargo arc check
```

A `warn` rule never fails the build; a reported `error` violation, a denied diagnostic, or a run that reaches no judgment at all does.
The report stands on stderr.
