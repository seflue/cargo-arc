# ADR-013: Use syn as Primary Analysis Backend

- **Status:** Active
- **Decided:** 2026-01-30
- Corrected 2026-10-13: the decision applies to the Rust language adapter, not to the analysis as a whole (ADR-029).

## Context

cargo-arc used rust-analyzer HIR (inherited from cargo-modules) for module enumeration and file path resolution. Research showed that both are replaceable by syn + filesystem walk.

## Decision

- The Rust adapter (ADR-029) analyzes module structure and use statements via `syn` (AST parsing)
- rust-analyzer HIR remains optional via Cargo feature `hir` and `--hir` flag
- We extract use statements via `resolve_use_tree` instead of the earlier text parser

## Rationale

- Measured speedup from over 50s to approximately half a second, with byte-identical SVGs
- ra_ap_* dependencies are eliminated in the default build
- The earlier text parser for use statements had bugs (phantom dependencies from use statements in string literals, faulty brace groups)
- syn-based extraction: one parse, correct resolution, no false positives

## Consequences

### Positive
- Analysis in sub-second instead of a minute
- Fewer dependencies in the default build
- More correct use-statement analysis

### Negative
- HIR path must be maintained separately
- syn cannot see resolved types (only syntactic analysis)
