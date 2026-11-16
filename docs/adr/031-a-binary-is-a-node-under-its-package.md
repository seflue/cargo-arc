# ADR-031: A Binary Is a Node under Its Package

- **Status:** Active
- **Decided:** 2026-11-16
- **Related:** ADR-005 (unified graph), ADR-018 (edge context)

## Context

A Cargo package builds a library, one or more binaries, or both, and each of them is a crate with a root file of its own.
A binary can depend on the library of its package, and the library never depends on a binary.

The graph holds one node per package, and the module trees of all root files of the package hang below that node.
A usage in the root file of a binary counts as a usage of the package node, and the same node stands for the items that the root file of the library defines.
When a library module uses an item from `lib.rs` and `main.rs` uses that module, the package node has an edge in each direction, and the cycle search reports cycles through it.
When a module that only a binary declares has the name of a library module, one of them remains.

With `--include-tests`, an integration test is already a node of its own under its package, with a crate root of its own.

## Decision

The root of a package is its library.
A package without a library and with exactly one binary has that binary as its root.

Every binary that is not the root is a node of its own under the package.
Its usages start at that node, and the modules that only it declares sit below it.
A package with several binaries and no library has a root without code of its own.

A module that the library and a binary both declare from the same file stands once, as a module of the library.

The node of a binary is named `<package>::{bin}::<target name>`.
The segment `{bin}` names the kind of target and is not a module.

A binary is production code.
Its edges count for rules, the cycle search and the diagram.

## Rationale

- No module of a library can depend on a binary. A node that joins the two gets edges in both directions, and with them cycles that the code does not contain.
- The package stays the unit that manifest edges, rule patterns and the overview of a workspace refer to. A top-level node per crate would give a package several top-level nodes, and the edge from a binary to its library would appear in no manifest.
- With binaries out of the graph, nothing would show what a binary uses.
- If a single binary without a library were not the root, every module of such a package would get a new name while its dependencies stay the same.
- A binary may have the same name as a module of the library. A segment without a marker would read as a module and could coincide with one.
- An integration test already follows this pattern.

## Consequences

- Cycles that ran through a usage in a binary and an item in the root file of the library disappear. Tangles can shrink or fall apart.
- A cycle between the root file of the library and one of its modules remains, and so does a cycle between a binary and its own modules.
- Path patterns and relative targets in `allow` reach a binary as `docs/RULES.md` describes, and a binary is the root of its crate for `crate` and `super`.
- `query` names the node of a binary as the source of a usage from it.
- A `layers` rule with `exhaustive` reports a binary until a layer names it.
- A baseline entry whose edge starts at a binary is reported again until it is frozen under the name of the binary.
- The diagram shows one row per binary under its package and above its modules. The hotspot map has no level for a binary.
- The package-level view does not show whether a dependency on another package comes from the library or from a binary.
- What a shared module needs from the root file of a binary is not in the graph.
- Integration tests keep the name `<package>::<name>` without a segment for the kind, so the two name forms differ.
- Examples and benches stay outside the graph.

## References

- `src/model.rs`: `BINARY_SEGMENT` and `binary_segments`.
- `src/analyze/syn_walker.rs`: `binary_node`, where the tree of a binary root file becomes the node.
- `docs/GLOSSARY.md`: the entries Binary, Node and Qualified node name.
- `docs/RULES.md` and `docs/QUERY.md`: patterns, relative targets of `allow`, and names in `query` output.
