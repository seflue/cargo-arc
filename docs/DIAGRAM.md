# The Diagram

`cargo arc` renders a workspace as a single self-contained SVG.
It draws a tree of crates with their modules nested inside, and arcs that trace the `use` dependencies between them.
The file carries its own styling and interaction, so any browser opens it and nothing is fetched from elsewhere.

```bash
cargo arc -o deps.svg
```

Without `-o` the SVG goes to stdout.

Terms used here are defined in [GLOSSARY.md](GLOSSARY.md).

## What you see

Every box is a crate or a module, nested by hierarchy.
Blue boxes are crates, orange boxes are modules, and grey boxes are external dependencies.

Every arc is a dependency between two boxes, and carries exactly one arc type:

- a **crate dependency**, declared in `Cargo.toml`,
- a **module dependency**, a `use` between modules,
- a **re-export**, an arc whose imports are all `pub use`.

Arcs on a circular dependency are drawn as cyclic edges in their own colour.
A collapsed node folds its children away, and their individual arcs merge into summary arcs on the collapsed box.

## Interacting

Hovering a node or an arc opens the sidebar for it while the pointer stays there.
Clicking pins it, and clicking the background clears it.
Selecting either one highlights its relationships in the diagram.

Double-clicking a node with children collapses or expands it, as does its toggle handle.
The toolbar button does the same for the whole tree and flips between **Collapse All** and **Expand All**.

### The sidebar for a node

The sidebar is divided into two sections.
The first lists the modules and crates that depend on the selected node, the second the ones it depends on.
Each entry expands into the symbols crossing that relation, and each symbol into the source locations that import it, as `file:line`.
The footer counts both directions, `N Dependents · M Dependencies`.

### The sidebar for an arc

The header names both endpoints, `from → to`.
Below it stand the symbols the arc carries, ordered by how many references each has, and under a symbol its locations.
The footer counts `N References · M Symbols`.

A crate dependency has no symbols of its own to show and reads `Cargo.toml dependency` instead.

Each symbol carries its consumer locality, which answers whether it could move closer to the modules using it.
A single consumer reads `only used by <module>`, consumers sharing an ancestor module read `used under <module>`, and consumers scattered across the crate read `widely used (N modules)`.

### The sidebar for a cluster

In cluster mode the sidebar describes the whole tangle rather than the one arc under the pointer.
It reads `Cluster · <crate>` with the extent of the tangle, `N modules · M cycles`.

## Searching

The toolbar holds a substring search over the diagram.
The scope buttons restrict it to crates, modules or symbols, and `All` searches everything.
Matches keep their colour while everything else dims, and the count beside the field says how many there are.

## Filters

The **View** menu holds one checkbox per filter.
Four of them cover arcs:

| Filter | Default |
|--------|---------|
| Show Crate Dependencies | on |
| Show Module Dependencies | on |
| Show Re-Export Dependencies | off |
| Show Circular Dependencies | on |

**External Dependencies** and **Transitive Dependencies** cover nodes and appear only when the run collected them.

Three of the four arc filters select on the arc's type.
The circular ones select on a property an arc carries in addition to its type, so an arc can fall under two filters at once.

An arc is shown when at least one arc filter covering it is on and the node filters leave both its endpoints standing.
Neither side overrides the other.
A node filter cannot force an arc back into view, and an arc filter cannot bring back an arc whose endpoint is hidden.

The circular-dependency checkbox does one thing beyond its filter.
It turns on **cluster mode**, which widens hovering, highlighting and styling from the single edge to the whole tangle it belongs to.
That mode is not a filter of its own.

## Test code

`--include-tests` widens the diagram to the test side: test modules, the crates only they reach, and the arcs between them.

```bash
cargo arc --include-tests -o deps.svg
```

Without it, modules under `#[cfg(test)]` stay unwalked and a crate that only a dev-dependency reaches does not appear at all.

## Re-exports

`--include-reexports` lets re-export arcs count as real dependencies, so a circle running through them is found and its arcs are drawn as cyclic edges.

```bash
cargo arc --include-reexports -o deps.svg
```

Without it such an arc is still drawn and still has its filter, and only stays out of the search.
A module writing `pub use` passes a name on rather than depending on it.
The flag governs `cargo arc check` the same way, where it decides which circular dependencies a `no-cycles` rule reports.

## Feature filtering

Show only the dependency subgraph for a specific Cargo feature:

```bash
# Show crates involved in the "web" feature (includes default deps)
cargo arc --features web -o web-deps.svg

# Exclude default deps - show ONLY the "web" feature graph
cargo arc --features web --no-default-features -o web-deps.svg
```

`--all-features` activates everything the workspace offers.

## External dependencies

Show which external crates your modules depend on:

```bash
# Direct external dependencies only
cargo arc --externals -o deps.svg

# Include transitive external dependencies
cargo arc --externals --transitive-deps -o deps.svg
```

External dependencies appear as separate nodes in the graph.
The sidebar distinguishes direct from transitive dependencies with distinct styling.

## Expand level

Start with deeper modules pre-collapsed to keep large workspaces readable:

```bash
# Show only crates (everything collapsed)
cargo arc --expand-level 0 -o deps.svg

# Show crates and their direct modules
cargo arc --expand-level 1 -o deps.svg
```

Nodes beyond the given depth start collapsed.
Click to expand interactively.
