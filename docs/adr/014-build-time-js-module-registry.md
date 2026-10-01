# ADR-014: Discover JS Modules at Build Time

- **Status:** Superseded: `js/` uses ES module imports, and `build.rs` bundles them with Bun
- **Decided:** 2026-01-30

## Context

JS modules were registered manually via `include_str!()` in `render.rs`. Forgotten modules led to `ReferenceError` only at runtime in the browser. This bug occurred repeatedly.

## Decision

JS modules declare `@module`, `@deps`, `@config` annotations in the first 5 lines. `build.rs` discovers the files automatically, sorts them topologically by dependencies, validates the declarations, and generates `js_modules.rs`.

## Rationale

- We catch "forgotten JS module" at build time instead of at runtime
- Topological sorting ensures the correct load order
- Annotation-based: metadata lives with the files they describe

## Consequences

### Positive
- Forgotten modules are detected at build time, not in the browser
- Load order is automatically correct
- New modules only need annotations, no manual registration

### Negative
- `build.rs` complexity increases
- Annotations must be maintained correctly
