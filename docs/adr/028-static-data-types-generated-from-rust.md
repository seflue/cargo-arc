# ADR-028: Generate the STATIC_DATA Types from the Rust Structs

- **Status:** Active
- **Decided:** 2026-10-10
- **Related:** ADR-006 (metadata embedded in the output)

## Context

Rust serializes `STATIC_DATA` for two pages, the arc diagram and the hotspot map, from serde structs.
The frontend is TypeScript and is type-checked against declarations of that data.
The declarations were written by hand, in one file, and described only the arc diagram's shape; the hotspot map's fields were missing.

The CSS class names are defined in Rust.
The frontend received some of them through a hand-maintained list in `STATIC_DATA` and wrote the rest as literals.

The hand-written declarations and the serialized output could diverge without a failing test, and they did.

## Decision

The TypeScript types of both pages' `STATIC_DATA`, and of the CSS class names, are generated from the Rust structs with ts-rs and checked in.
Only the placeholder constants that the page scripts receive at render time are declared by hand.

The class names reach the frontend as one map built from the whole Rust struct, nested as the struct is.

Each page is its own type-check project, as each page is its own bundle.
In each project `STATIC_DATA` has the type of its page.

## Rationale

- A type generated from the source of the output cannot diverge from it. A hand-written declaration checked against sample output covers only the fields the sample fills.
- The class names are generated too, so a typo in a class access fails the type check. By hand they would have to be declared a second time.
- A checked-in sample output changes with every layout change; the generated types change only when a type changes.
- Checked in, so a change to a struct shows up in the review diff as a change to the type the frontend sees.
- Nested, because several field names occur in more than one group of classes. A flat map would need renames and a uniqueness check.
- One project per page instead of a union type, because otherwise every reader of `STATIC_DATA` would first have to narrow down which page is running.
- ts-rs reads the serde attributes, so the generated type follows the names serde writes.

## Consequences

- ts-rs is a new dependency, and its derive also sits on types outside rendering, in the layout and in the hotspot tree.
- The type files are regenerated before every type check. CI fails when a regenerated file differs from the checked-in one.
- The `STATIC_DATA` stubs in the JavaScript tests are not type-checked, because the test files are not part of either project.
- Every class access in the frontend names its group, as in `C.nodes.collapsed`.

## References

- `js/generated/`: the generated types.
- `js/arc_page.d.ts`, `js/hotspot_page.d.ts`: `STATIC_DATA` per page; `js/globals.d.ts`: the placeholder constants.
- `tsconfig.arc.json`, `tsconfig.hotspot.json`: the two type-check projects.
- `src/render/static_data.rs`, `src/render/hotspots.rs`, `src/render/constants.rs`: the serialized structs.
