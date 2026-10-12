# ADR-010: Encapsulate DOM Access behind DomAdapter

- **Status:** Active
- **Decided:** 2026-01-27
- Corrected 2026-10-13: the frontend is TypeScript, and the adapter covers element access, not all DOM access. Viewport size, scroll position and the theme are accessed directly.

## Context

`svg_script.js` (now `svg_script.ts`) was hard to test: DOM manipulation and business logic were intertwined. Pure function extraction alone would have made only a small portion of the code testable, since the majority contains DOM accesses.

## Decision

We encapsulate access to the diagram's elements behind a `DomAdapter` interface. `createMockDomAdapter()` for tests (call tracking), `DomAdapter` for the browser (delegates to `document.*`).
Viewport size and scroll position, the theme on the document root and the stored theme choice are accessed directly.

## Rationale

- Mock injection makes the majority of the code testable, not just pure functions
- Unit tests run with Bun/Node without a browser
- Lightweight: fake elements instead of a full JSDOM

## Consequences

### Positive
- Significantly more code is unit-testable
- Tests are fast (no browser environment)

### Negative
- Adapter must be extended for new DOM operations
- Mocks could diverge from real DOM behavior
