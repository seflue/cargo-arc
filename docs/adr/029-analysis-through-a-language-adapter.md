# ADR-029: Analyze Through a Language Adapter

- **Status:** Active
- **Decided:** 2026-10-11
- **Related:** ADR-002 (four-stage pipeline), ADR-013 (syn as the Rust backend)

## Context

The analysis was written for Rust.
It read the workspace through `cargo metadata` and parsed it with syn or rust-analyzer HIR.
The graph read Cargo's dependency kinds itself to set the context of an edge to an external package.

Code outside the analysis also knew the language.
Volatility, line counts and the rerun on save recognized `.rs` files and `Cargo.toml` themselves, and the page called every package a crate.

A second language was planned.

## Decision

A language adapter does the analysis.
An adapter reads a project and delivers its packages, one module tree per package, and its external packages when they are asked for.
cargo-arc builds the dependency graph from what an adapter delivers and from nothing else.

Each adapter has a language profile.
The profile names the words the page uses for a package and for a manifest, which files are source files, which files are manifests, and the language in which lines are counted.
Code outside the analysis that depends on the language reads the profile.

The model holds only types that do not depend on a language.
Whatever belongs to one language stays in its adapter.
For Rust that is everything from Cargo, including how Cargo's dependency kinds become the context of an edge.

The Rust adapter chooses between syn and rust-analyzer HIR.

ADR-007, ADR-013, ADR-020 and ADR-022 describe how the Rust adapter analyzes, and apply to it only.

## Rationale

- ADR-002 separated the stages so that a backend could be replaced. The adapter is where that happens, and syn versus HIR becomes a choice inside the Rust adapter.
- The graph, the layout, the rendering and the cycle analysis are not specific to Rust. Built on language-neutral types, they need no change for a second language.
- One profile replaces a Rust check in each consumer. Volatility, line counts, the rerun on save and the page's wording read the same record, so a new language sets them in its profile.

## Consequences

- A new language needs an adapter and a profile.
- The model and the graph name packages, not crates. The page shows the profile's word, "crate" for Rust.
- The options for Cargo features and for rust-analyzer HIR are settings of the Rust adapter.
- An adapter for another language makes its own choices on the questions ADR-007, ADR-013, ADR-020 and ADR-022 answer for Rust.

## References

- `src/analyze/mod.rs`: the `LanguageAdapter` trait.
- `src/analyze/rust.rs`: the Rust adapter and its profile.
- `src/model.rs`: `Analysis`, what an adapter delivers, and `LanguageProfile`.
- `src/cli.rs`: `build_dependency_graph`, where the graph is built from an adapter's output.
