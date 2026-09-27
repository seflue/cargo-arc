# The Hotspot Map

The hotspot map shows which files in a workspace are large and which change often.
A file that is both is expensive to maintain and a good place to start a refactoring or a review.

```bash
cargo arc hotspots -o hotspots.svg
```

Without `-o` the SVG goes to stdout.

Terms used here are defined in [GLOSSARY.md](GLOSSARY.md).

## What it measures

Size is a file's lines of code, without comments and blank lines.
Change is the number of commits that touched the file in the last six months.
Every file is a circle of that size, nested inside its module and crate, and its color runs from cold to hot with its commits.
A crate or module adds up its files, so the map also tells which part of the workspace holds the size and the churn.

`--volatility-months` sets a different window, for example `cargo arc --volatility-months 12 hotspots`.
Without a git repository, or with `--no-volatility`, the map shows size only.

## Hotspots

The files with the highest lines of code times commits are the hotspots, ten by default.
The sidebar lists them in rank order, and `--hotspots` sets how many.

## Beside the arc diagram

`cargo arc ui` serves the map next to the arc diagram, and a selection carries over when you switch between the two.
A hotspot found on the map can be looked up in the arc diagram to see what depends on it, and back.
A selected file can be opened in the editor, the map follows the file the editor shows, and it recomputes when a file is saved, as the arc diagram does ([ARC_DIAGRAM.md](ARC_DIAGRAM.md#following-the-editor)).
