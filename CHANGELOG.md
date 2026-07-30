# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->

## [Unreleased]

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
