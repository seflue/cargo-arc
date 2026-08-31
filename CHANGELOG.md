# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->

## [Unreleased]

### Added

- Rules in `arc-rules.toml` take an `except` list of permanently allowed edges,
  each `{ from, to, reason }`, with the same `*` and `**` patterns the rules
  themselves use. An allowed edge is not reported; under a `no-cycles` rule it
  is removed before the search, so cycles built through it never form.
- `arc check --show-silenced` lists the violations that an `except` entry
  allows. Without it a run only counts them.
- `arc check --generate-baseline` writes `arc-baseline.toml` next to
  `arc-rules.toml`, freezing the violations a project already has so that only
  new ones are reported. A normal run never writes the file. An entry carries
  the rule name and names one dependency edge together with the symbols
  tolerated on it. A cycle is frozen when every one of its edges is, so an entry
  outlives a refactor that reshapes the cycle around it, and an edge that gains
  a symbol is reported again. Generation is refused while an `except` pattern
  matches no module, and `--show-silenced` lists frozen violations alongside
  allowed ones. When a tangle holds both frozen and reported cycles, its ranked
  edge list covers only the reported ones, so no listed edge stands for a cycle
  the baseline froze.
- Rule names must now be unique across all rule types; `arc-rules.toml` is
  rejected when two rules share a name.
- A `[diagnostics]` section in `arc-rules.toml` reports gaps in the
  configuration itself: `unlayered-node` for a node an `exhaustive` `layers`
  rule leaves in no position (the rule never asks where it belongs),
  `unmatched-baseline-entry` for a frozen violation that no longer occurs,
  `unmatched-except` for an `except` pattern that matches no module,
  `unmatched-pattern` for a rule pattern that matches no module. Each is set to
  `allow`, `warn` or `deny`; `deny` fails the run. `unmatched-pattern` and
  `unlayered-node` default to `deny`, for the same failure shape: a rule whose
  pattern misses, or a node it leaves unsorted, checks nothing and leaves the
  run green; the others default to `warn`.
  `unlayered-node` also takes an `except` list of qualified node names that
  stand outside the architecture on purpose, each taking the modules below it
  with it, and is written either as `"warn"` or as
  `{ level = "warn", except = ["xtask"] }`.
- `arc check` states its judgment on stdout, one line per checked rule and one
  for the configuration: `<rule> ok: 0 errors, 0 warnings, 0 allowed, 0 frozen`,
  with `WARN` and `FAILED` for the other two outcomes. The line is printed
  whether or not the rule fired, so a clean run says so instead of falling
  silent. Allowed and frozen violations are counted apart, because one is meant
  to stay and the other to shrink. The report itself stays on stderr.
- An entry in a `layers` rule may name several patterns instead of one, written
  as `["adapter_a", "adapter_b"]` in place of `"adapter_a"`. Crates of equal
  rank share one entry, so the order they are listed in no longer forbids edges
  between them. Edges leaving the rank still follow the rule's direction. A
  plain string is a rank of one, so existing rules are unaffected.
- A `layers` rule takes `exhaustive = true` (default `false`) to declare itself
  complete over what it addresses: its crate patterns claim every workspace
  crate, its module patterns every module of the crates those patterns reach.
  Each gap is reported as `unlayered-node`, naming the topmost node of a
  containment chain rather than every module below it, and each rule is judged
  on its own. Without the field a rule says nothing about the nodes it does not
  name. `exhaustive = true` beside the catch-all layer `*` is refused when the
  file loads, because the catch-all would satisfy the claim by construction.
- A `*` in a pattern now matches inside a name part instead of only as a whole
  segment, so one entry such as `*application*` covers every crate whose name
  carries the layer, rather than each being listed by hand in every rule. A `*`
  never crosses `::`, `**` still stands for whole segments, and `*` also matches
  the empty string, so `nwa_application*` covers a crate named `nwa_application`
  as well. The existing forms `**`, `core::**` and `core::*` denote the same sets
  as before. Wildcards hold in `except` too.
- A `layers` rule takes `*` as a position of its own, the catch-all layer, which
  holds every node the rule's other positions leave. That is how "all of this is
  one layer, except these" is written now that patterns can overlap by
  construction. A catch-all that stays empty is reported as `unmatched-pattern`.

### Changed

- A `no-cycles` rule now reports one violation per tangle instead of one per
  elementary cycle, using the same block form as a run without
  `arc-rules.toml`. Each block is headed by the location the tangle sits in,
  the shared module prefix of its members.
- The rule header on stderr, `error[rule-type]: rule-name`, is now printed once
  per rule instead of once per violation, and the violations stand indented
  below it: the edge on a `= ` line, its source locations on `--> ` lines under
  that. Before, a rule with six violations repeated its header six times.
- A pattern in `arc-rules.toml` starts at the crate name, and the `crate::`
  prefix is no longer stripped from it. A rules file covers the workspace, so
  `crate::` has no referent there; `crate::domain` now matches nothing, like
  `self::` and `super::` and like any typo, and `unmatched-pattern` reports it.
  Existing rules that use the prefix must drop it.
- A run that reached no judgment now exits 2 instead of 1. A rules file, a
  baseline or a workspace that fails to load is no longer indistinguishable from
  an architecture violation. Exit 1 keeps its meaning: a rule reported an error,
  or a diagnostic set to `deny` fired.
- A run without `arc-rules.toml` is a rule run like any other: a missing file
  means an implicit `no-cycles` rule over the whole workspace, reported under
  the name `no cycles`, and it carries the baseline and the `except` list the
  same way a written rule does. Before, it took a path of its own that knew
  neither. A `no-cycles` rule in the file replaces the implicit one instead of
  joining it, so a narrower `scope` can allow cycles the implicit rule would
  forbid; the name `no cycles` is reserved unless the file checks cycles itself.
  The summary line that counted cycles and tangles went with the old path.
- A key that is not part of the format is now an error, and the message names
  the key. Before, such a key was ignored: `[diagnostic]` for `[diagnostics]`,
  `scpoe` for `scope`, or `excpet` inside `unlayered-node` all loaded fine and
  switched nothing on.
- A module pattern now matches its node and everything below it, the way a crate
  pattern always has, so `storage::model` covers the modules under it and the
  pair `storage::model` plus `storage::model::**` collapses to the first alone.
  `::**` still means the subtree without the node and `::*` still means the
  direct children; only the bare form widened. A rule written as such a pair
  keeps its meaning. The change reaches every rule type, `except` included, where
  a pattern that is now wider grants a wider permanent allowance.
- Two positions of a `layers` rule that match the same node now fail the run,
  which names the node, both positions and the rules file. Before, the later
  position won without a word. Once patterns carry wildcards an overlap stops
  being a typo and becomes the ordinary case, so it has to be said out loud. The
  catch-all layer `*` is exempt: holding what the other positions leave is what
  it is for.
- A `layers` rule now checks dependencies rather than written edges: one that
  reaches its target over nodes the rule sorts into no position counts like a
  direct one and is reported under the pair at its ends, with its hops listed
  below it. Before, such a dependency vanished from the check, so an order that
  contradicts the code could stay green. A project that passes on 0.3.1 can turn
  red on this; `arc check --generate-baseline` freezes what is there. The
  baseline key and `except` both address the pair, so an entry stays valid when
  the dependency later runs over another node. The diagnostic for an unsorted
  node now says its own place goes unchecked, which is what is left.

### Fixed

- A rule with `severity = "error"` was downgraded whenever `[config]` set
  `default_severity` to something else, which made an error rule impossible to
  write under `default_severity = "warn"`.
- An error message now carries the whole chain instead of only its outermost
  layer. A run aimed at a directory without a `Cargo.toml` reported `Failed to
  run cargo metadata` and nothing else; it now names the manifest path it
  targeted and passes on what cargo itself said.

### Removed

- The hidden `--check` flag, the pre-0.3.0 way of asking for a cycle check.
  `arc check` replaces it and accepts the same common arguments; without an
  `arc-rules.toml` it falls back to the implicit `no cycles` rule, so a run
  that passed the flag keeps its verdict.

## [0.3.1] - 2026-07-30

### Changed

- `arc check` no longer reports how many edges must be removed to break the
  cycles. The ranked edge list instead names its sort order and states which
  cycles the listed edges cover.

### Fixed

- Crates whose root file declares no submodules stayed out of the diagram in
  workspaces that use hyphenated package names or renamed dependencies.
- Modules were missing for crates whose library or binary target sets a `path`
  other than `src/lib.rs` or `src/main.rs`.
- A workspace entry point that nothing depends on, such as a thin binary, was
  pruned from the diagram along with the test-only crates.

## [0.3.0] - 2026-07-28

### Added

- Add `arc check` command, which validates a workspace against architecture
  rules defined in `arc-rules.toml`.
- Add cycle view to diagram. Allows selecting a cluster and listing its cycles
  in the sidebar.

### Changed

- Import resolution now handles re-exports, glob imports and self-imports
  correctly. It also fixes the performance problem stemming from too many
  elementary cycles.

### Fixed

- Removed toolbar and sidebar from static SVG.

## [0.2.2] - 2026-03-19

### Fixed

- Cycle detection hanging on large graphs (now capped)

## [0.2.1] - 2026-03-13

### Added

- Single-file crates rendered in workspace diagrams

### Changed

- Node and arc filter toggling unified
- Shared show logic extracted in sidebar
- Shared arc-row logic extracted in cycle sidebar
- Layer dispatch replaced with table lookup
- Virtual arcs rendered in single pass
- Redundant no-op wrappers removed, accessor methods introduced
- `classList.toggle` used instead of manual class manipulation

### Fixed

- Single-file crates invisible in workspace diagrams (anchor detection required Contains-edges)

## [0.2.0] - 2026-03-01

### Added

- `--expand-level` flag to start with deeper modules pre-collapsed
- External dependency visualization with `--externals` flag
- `--transitive-deps` flag to include transitive external dependencies
- Direct vs transitive dependency distinction (separate visual styling)
- Sidebar shows external dependencies as flat rows with pill-styled badges
- Click sidebar badges to navigate to the corresponding graph node
- Toggle (+/−) on sidebar node badges for expand/collapse
- Sidebar refreshes when nodes are expanded or collapsed
- Search dimming extended to arcs and external dependency nodes
- Toolbar toggle for transitive dependency filtering

### Changed

- Sidebar entries sorted by tree position
- Sidebar styling aligned with tree nodes (font-weight, badge widths)
- Sidebar hides during navigation scroll
- Toolbar checkboxes moved into dropdown menu

### Fixed

- SVG viewport clipping after expand/collapse
- Missing arcs after expanding all initially collapsed nodes
- Sidebar collapse broken with external dependencies present
- Sidebar scroll jitter on short navigation jumps
- Sidebar flicker during badge navigation scroll
- Sidebar collapse-all needing two clicks
- Collapse-all action not matching button label
- Missing selection styling on sidebar header
- Phantom highlight after background deselect
- Hover lag during active search
- Toolbar overflowing browser viewport width
- Dimmed nodes not clickable when pinned
- Sidebar not filling available viewport height

### Performance

- DOM element cache for O(1) hover lookups
- Debounced hover highlights to reduce render churn
- Cached filter-hidden node set per toggle cycle
- Diff-based search highlight updates (skip stable matches)

## [0.1.0] - 2026-02-23

Initial public release.

### Added

- Interactive workspace dependency visualization as arc diagram
- Collapsible crates and parent modules
- Highlight relationships on hover and click
- Symbol-level tooltips (imported symbols per dependency)
- Cross-crate dependency edges via `syn`-based `use` statement parsing
- Feature-based dependency graph filtering
- Cycle detection via Johnson's algorithm with badge navigation
- Search with dimming of non-matching nodes
- HTML report generation (single self-contained file)

<!-- next-url -->
[Unreleased]: https://github.com/seflue/cargo-arc/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/seflue/cargo-arc/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/seflue/cargo-arc/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/seflue/cargo-arc/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/seflue/cargo-arc/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/seflue/cargo-arc/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/seflue/cargo-arc/releases/tag/v0.1.0
