# ADR-020: Resolve Re-Exports to Determine True Dependencies

- **Status:** Active
- **Decided:** 2026-02-23

## Context

Rust developers freely choose between import variants: `use super::Item`, `use crate::parent::Item`, or `use super::sibling::Item`. All three are idiomatic — large projects like tokio, rustc, and the standard library mix them freely, and no style guide or Clippy lint prescribes a particular import style.

cargo-arc analyzes use statements syntactically to determine dependencies between modules. Without re-export resolution, `use super::FeatureConfig` creates an edge to the parent module even when `FeatureConfig` is only re-exported there and actually originates from a sibling module. This produces false-positive cycles and obscures real architectural violations.

## Decision

cargo-arc resolves re-exports to determine the true dependencies between modules. Every import is traced back to the original `pub` item, so the dependency graph reflects actual module relationships — regardless of import style.

## Rationale

- Import style is a matter of taste and architecturally irrelevant — what matters is the actual dependency direction
- Without resolution, edges point to the parent module instead of the originating module — this distorts the graph in both directions: real dependencies disappear, and spurious dependencies to the parent module appear
- Consistent with practice in large Rust projects: architecture rules concern module layering, not import syntax

## Consequences

### Positive
- Architecture rules detect violations regardless of the developer's import style
- Cycle detection operates on the real dependency graph, not on import paths
- Import refactorings (e.g. `use super::X` to `use crate::parent::X`) do not change the graph as long as the underlying dependency remains the same

### Negative
- Resolving re-export chains adds complexity to the analysis step

