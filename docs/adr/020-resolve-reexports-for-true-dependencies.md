# ADR-020: Resolve Re-Exports to Determine True Dependencies

- **Status:** Active
- **Decided:** 2026-02-23
- Corrected 2026-10-13: an import is traced to the module that defines the item it names, not only to an original `pub` item. This includes names a module reaches through a private `use` in one of its ancestors.

## Context

Rust developers freely choose between import variants: `use super::Item`, `use crate::parent::Item`, or `use super::sibling::Item`. All three are idiomatic — large projects like tokio, rustc, and the standard library mix them freely, and no style guide or Clippy lint prescribes a particular import style.

cargo-arc analyzes use statements syntactically to determine dependencies between modules. Without re-export resolution, `use super::FeatureConfig` creates an edge to the parent module even when `FeatureConfig` is only re-exported there and actually originates from a sibling module. This produces false-positive cycles and obscures real architectural violations.

## Decision

cargo-arc resolves re-exports to determine the true dependencies between modules.
Every import is traced back to the module that defines the item it names, so the dependency graph reflects actual module relationships regardless of import style.

Rust makes a private `use` in a module visible to that module and to its descendants.
A descendant can therefore name an item through an ancestor that only imports it.
Such an import is traced to the defining module as well.

## Rationale

- Import style is a matter of taste and architecturally irrelevant — what matters is the actual dependency direction
- Without resolution, edges point to the parent module instead of the originating module — this distorts the graph in both directions: real dependencies disappear, and spurious dependencies to the parent module appear

## Consequences

### Positive
- Architecture rules detect violations regardless of the developer's import style
- Cycle detection operates on the real dependency graph, not on import paths
- Import refactorings (e.g. `use super::X` to `use crate::parent::X`) do not change the graph as long as the underlying dependency remains the same
- A private `use` or private glob import in an ancestor is followed only for the ancestor's descendants, the modules that Rust lets see it

### Negative
- Resolving re-export chains adds complexity to the analysis step
- When a descendant names an item that the ancestor's private `use` takes from an external crate, the reference produces no edge
- A `use` with `pub(in path)` visibility is not followed, and neither is a `use` inside a module declared inline with a `mod` body

