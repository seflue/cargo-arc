# cargo arc query

`cargo arc query <COMMAND>` prints data from the analysis on stdout, one record per line, for scripts and agents.
The output says what it describes: it opens with header lines, followed by the column names and then the records.
The flags shared with the diagram, such as `--include-tests` or `--features`, go before `query` and mean the same as for the diagram.
`--json` goes after `query`, before or after the subcommand.

## TSV

TSV is the default.
Each header line starts with `#` and holds one field as `# key: value`.
Every subcommand's output opens with these fields:

| Key | Value |
|-----|-------|
| `cargo-arc-version` | The version of the cargo-arc that wrote the output. |
| `format-version` | The version of this format. It is raised whenever a column or a header field is added, removed, or changes its meaning. |
| `workspace-root` | The absolute path of the analyzed workspace. Paths in the records are relative to it. |
| `git-commit` | The commit `HEAD` names in the repository holding the workspace, suffixed `-dirty` when a tracked file has uncommitted changes, as `git describe --dirty` marks it. `none` outside a repository or before its first commit. |
| `flags` | The flags that shape the data, as space-separated `name=value` pairs with the value in effect, defaults included. A list is comma-separated, and an empty list leaves the value empty. |

The subcommand's own fields follow them.

The line after the header holds the column names, separated by tabs.
Each following line is one record, its fields separated by tabs.
A subcommand with no records prints the header and the column line and exits with 0.

A field never splits its line: in header values and in records, a backslash is written as `\\`, a tab as `\t`, a newline as `\n` and a carriage return as `\r`.
A reader undoes the escapes after splitting the line at the tabs.

## JSON Lines

`--json` prints the same content as JSON Lines, one JSON object per line.
The first object holds the header fields under the same keys and in the same order as the TSV header, followed by `columns`, the list of column names.
A value the TSV writes as `none` is `null`, a number is a number, and `flags` is an object.
Each following object is one record, keyed by the column names in their order.
JSON strings carry their own escapes, so the TSV escapes do not apply.

## query hotspots

`query hotspots` lists the top N [hotspots](GLOSSARY.md#hotspot) in ranking order, five without `-n, --top <N>`.
The ranking is the one `cargo arc hotspots` draws.

`query hotspots <PATH>...` lists exactly the named files instead, each with its rank among all ranked files, files that are no hotspot included.
The ranked files are the files of the module tree; test files only with `--include-tests`.
The paths are relative to the workspace root, and the records come in ranking order, whatever order the paths were given in.
A path that is not in the ranking ends the run with an error naming it, and `-n` together with paths is a usage error.

| Column | Value |
|--------|-------|
| `rank` | The file's position in the ranking, counting from 1. |
| `path` | The file, relative to the workspace root. |
| `code-lines` | The file's [code lines](GLOSSARY.md#code-lines). |
| `commits` | The commits in the window that touched the file. |

The header adds these fields after `flags`:

| Key | Value |
|-----|-------|
| `top` | N, or `none` when files are named. |
| `ranking` | The ranking rule: code lines times commits, highest first. A file scoring zero is no hotspot and never among the top N, but has a rank when named. |
| `ties` | How equal scores are ordered: by the file's path relative to the workspace root. |
| `code-lines` | What a code line is: tokei's code count, without comments and blank lines. |
| `commits` | Which commits count: distinct commits whose commit time lies in the window and that add, modify, rename or copy the file (`git log --diff-filter=AMRC`). Merge commits and the deletion of a file do not count. |
| `window-start` | The UTC date the window starts on, `--volatility-months` before the run. The window starts at the run's time of day on that date. |
| `window-end` | The UTC date of the run, the window's end. `GIT_TEST_DATE_NOW` sets the run's time as it does for git. |

Without volatility data, under `--no-volatility` or outside a git repository, `window-start` and `window-end` are `none`, every file has zero commits, and the top N lists no file.

## query edges

`query edges` lists every symbol that crosses an [edge](GLOSSARY.md#edge) between modules, one record per edge and symbol.
A package stands for its root module, so an edge can start or end at a package.
The records are the arcs of the diagram and the symbols listed under each, under the same flags.
The [bare imports](GLOSSARY.md#bare-import) of an edge name no symbol and share one record with an empty `symbol`.
A [manifest edge](GLOSSARY.md#manifest-edge) that no import backs has no record.
The records are ordered by `from`, then `to`, then `symbol`.

| Column | Value |
|--------|-------|
| `from` | The importing module, as its [qualified node name](GLOSSARY.md#qualified-node-name). |
| `to` | The module or package the edge points to, as its qualified node name. |
| `symbol` | The name the imports take from `to`; empty for the bare imports. |
| `usage-categories` | The [usage categories](GLOSSARY.md#usage-category) of the symbol's [usages](GLOSSARY.md#usage) in the importing files, in the order `types`, `values`, `functions`. Empty when the files never use the symbol, as when they only re-export it, and always for the bare imports. |
| `re-exported` | `true` when every import line that names the symbol is a `pub use` and the importing file refers to no name on that line otherwise, as the diagram's `viaReexport`; `false` otherwise and for the bare imports. A `pub use` of several names whose one name the file also uses is no re-export for any of them. |
| `locations` | The imports that name the symbol, as `path:line`, the path relative to the workspace root. |
| `cycles` | The number of [representative cycles](GLOSSARY.md#representative-cycle) through the edge, the same for each symbol on it. A [re-export](GLOSSARY.md#re-export) lies on none unless `--include-reexports`. |

In TSV, `usage-categories` and `locations` are comma-separated; JSON gives both as lists.

The header adds these fields after `flags`:

| Key | Value |
|-----|-------|
| `records` | What a record is: one per edge and symbol, the bare imports in one record, no record for a manifest edge without imports. |
| `order` | How the records are ordered. |
| `usage-categories` | What the `usage-categories` column holds. |
| `re-exported` | What the `re-exported` column holds. |
| `locations` | What the `locations` column holds. |
| `cycles` | What the `cycles` column counts. |
| `not-shown` | What an edge does not show: method calls on a receiver, field access, usages inside the same module, and test usages unless `--include-tests`. |
