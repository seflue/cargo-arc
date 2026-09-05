# cargo-arc

[![Crates.io](https://img.shields.io/crates/v/cargo-arc)](https://crates.io/crates/cargo-arc)
[![CI](https://github.com/seflue/cargo-arc/actions/workflows/ci.yml/badge.svg)](https://github.com/seflue/cargo-arc/actions/workflows/ci.yml)

`cargo arc` draws a Cargo workspace as a collapsible arc diagram in SVG, a tree of your crates and their modules connected by arcs that trace the `use` dependencies between them.
`cargo arc check` holds the same workspace against architecture rules you write down, and fails the build when one breaks.

## Installation

```bash
cargo install cargo-arc
```

Requires a stable Rust toolchain.

## Quick Start

```bash
# In any Cargo workspace:
cargo arc -o deps.svg   # the diagram
cargo arc check         # the architecture rules
```

Open the generated SVG in a browser.

## What You See

Your workspace shows up as a tree — crates with their modules nested inside.
Arcs between nodes show where dependencies exist.

- **Boxes** — crates and modules, nested by hierarchy
- **Arcs** — dependencies between any two nodes
- **Collapse** a node to fold its children — individual dependencies merge into summary arcs
- **Select** a node or arc to highlight its relationships
- **Cycles** — circular dependencies are detected and highlighted

## Architecture Rules

Circular dependencies are forbidden by default, and a workspace without a rules file is checked against that one rule.
`arc-rules.toml` states which crate may depend on which, which modules or crates may never depend directly on which, and where circular dependencies are permitted after all.
`cargo arc check` reports what it finds, and a reported error makes it exit non-zero, so it belongs in CI next to the test suite.

A workspace that has grown for years rarely comes out clean on the first run.
`cargo arc check --generate-baseline` freezes what exists today, so the run turns green and reports everything added after it.

## What the Run Does Not See

`cfg` attributes are currently not evaluated, so the graph is the union over all configurations.
A module under `#[cfg(feature = "hydrate")]` or `#[cfg(target_os = "windows")]` is walked on any machine and under any feature set.
Two modules whose attributes exclude each other are currently reported as one tangle, although the reported circular dependency exists in no build.
Modules under `#[cfg(test)]` are the exception and stay out unless `--include-tests` is passed.

A `mod` declaration inside a macro invocation is not analysed.
Where a crate writes `feature_gate! { pub mod runtime; }`, `runtime` and every module below it are missing from the graph, and a rule naming it reports `unmatched-pattern`.

A pattern reaches the crates under analysis and the modules in them.
A crate the workspace only depends on is not a node, so a rule about a third-party crate matches nothing.

A green run says the rules held over the modules the run walked, and a module a macro declares is not one of them.

## Documentation

- [docs/DIAGRAM.md](docs/DIAGRAM.md) — the diagram: what it draws, interaction, filters, flags
- [docs/RULES.md](docs/RULES.md) — the rules: how to start, the baseline, the file format
- [docs/GLOSSARY.md](docs/GLOSSARY.md) — the terms of the domain

## Similar Projects

- [cargo tree](https://doc.rust-lang.org/cargo/commands/cargo-tree.html) — built-in textual dependency tree (crate-level)
- [cargo-modules](https://github.com/regexident/cargo-modules) — module tree and dependency visualization
- [cargo-depgraph](https://github.com/jplatte/cargo-depgraph) — crate-level dependency graph as DOT with color-coded dependency kinds
- [cargo-coupling](https://github.com/nwiizo/cargo-coupling) — coupling analysis based on Khononov's framework

## References

The arc diagram layout is inspired by Martin Wattenberg's [Arc Diagrams: Visualizing Structure in Strings](http://hint.fm/papers/arc-diagrams.pdf) (IEEE InfoVis 2002).

## Development

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for project structure and architecture decision records.

Requires [Just](https://github.com/casey/just) as task runner.

```bash
just build
just test    # Rust + JS
just lint    # clippy + format check
just fmt
```

## License

MIT OR Apache-2.0
