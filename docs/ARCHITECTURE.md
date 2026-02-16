# Architecture

## Project Structure

```
src/
├── analyze/
│   ├── mod.rs         # Coordination + public API
│   ├── use_parser.rs  # Use statement extraction (syn)
│   ├── hir.rs         # HIR module analysis (ra_ap_hir)
│   ├── filtering.rs   # Dependency classification + filtering
│   ├── syn_walker.rs  # Module discovery (syn + filesystem)
│   ├── backend.rs     # Analysis backend abstraction
│   └── workspace.rs   # Workspace analysis (cargo_metadata)
├── model.rs     # Shared data structures
├── graph.rs     # Dependency graph building (petgraph)
├── layout.rs    # Tree layout algorithm
├── render.rs     # SVG generation
├── volatility.rs  # Git history volatility analysis
├── js_registry.rs # JS dependency validation (build.rs)
├── cli.rs         # CLI interface (clap)
├── lib.rs       # Public API exports
└── main.rs      # Entry point
```

## Architecture Decision Records

| # | Title | Status | Date |
|---|-------|--------|------|
| 001 | [Use Deterministic Tree Layout with Dependency Arcs](adr/001-deterministic-tree-layout.md) | Active | 2026-01-20 |
| 002 | [Structure Processing as Four-Stage Pipeline](adr/002-four-stage-pipeline.md) | Active | 2026-01-20 |
| 003 | [Develop as Single Crate with Module Separation](adr/003-single-crate-with-module-separation.md) | Active | 2026-01-20 |
| 004 | [Generate SVG Directly in Rust](adr/004-direct-svg-generation.md) | Active | 2026-01-20 |
| 005 | [Model Hierarchy and Dependencies in Unified Graph](adr/005-unified-graph-with-contains-edges.md) | Active | 2026-01-20 |
| 006 | [Embed All Metadata in Self-Contained SVG](adr/006-self-contained-svg-with-embedded-metadata.md) | Active | 2026-01-20 |
| 007 | [Filter Features via cargo_metadata resolve](adr/007-feature-filtering-via-cargo-metadata-resolve.md) | Active | 2026-01-24 |
| 008 | [Enforce Z-Order via Named SVG Layers](adr/008-svg-layer-z-order.md) | Active | 2026-01-24 |
| 009 | [Use tracing for Structured Logging](adr/009-tracing-for-structured-logging.md) | Active | 2026-01-26 |
| 010 | [Encapsulate DOM Access behind DomAdapter](adr/010-dom-adapter-pattern.md) | Active | 2026-01-27 |
| 011 | [Separate State and Derived Data in Frontend](adr/011-state-derived-separation.md) | Active | 2026-01-29 |
| 012 | [Rebuild DOM on Collapse Instead of Hide/Show](adr/012-dom-rebuild-on-collapse-expand.md) | Active | 2026-01-29 |
| 013 | [Use syn as Primary Analysis Backend](adr/013-syn-as-primary-analysis-backend.md) | Active | 2026-01-30 |
| 014 | [Discover JS Modules at Build Time](adr/014-build-time-js-module-registry.md) | Active | 2026-01-30 |
| 015 | [Use foreignObject HTML for SVG Panels](adr/015-foreign-object-html-for-panels.md) | Active | 2026-01-31 |
| 016 | [Replace Tooltip with Persistent Sidebar](adr/016-sidebar-replaces-tooltip.md) | Active | 2026-01-31 |
| 017 | [SCC Condensation for Deterministic Cycle Resolution](adr/017-scc-condensation-for-cycle-resolution.md) | Active | 2026-02-11 |
| 018 | [EdgeContext Enum for Production and Test Context on Edges](adr/018-edge-context-enum.md) | Active | 2026-02-11 |
| 019 | [Detect Elementary Cycles via Johnson's Algorithm](adr/019-elementary-cycles-via-johnsons-algorithm.md) | Active | 2026-02-16 |
