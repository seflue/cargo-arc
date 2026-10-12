# Neovim Lua Plugins

`cargo arc` also analyzes a Neovim plugin written in Lua.
The arc diagram and the hotspot map work as they do for a Cargo workspace, and the page says *namespace* where it says *crate* for Rust.
This page lists what is different for a Lua plugin.
Everything it does not mention works as the other documents describe it.

```bash
# In the plugin's repository:
cargo arc -o deps.svg
cargo arc hotspots -o hotspots.svg
```

Terms used here are defined in [GLOSSARY.md](GLOSSARY.md).

## Which language a run analyzes

A run analyzes one language.
A directory that holds `lua/` and no `Cargo.toml` is a Lua plugin.
A directory with a `Cargo.toml` is a Rust workspace, even when it also holds `lua/`.

`--manifest-path` takes the plugin's directory.
Without it, the run looks at the working directory, unless a `Cargo.toml` lies there.
A directory with neither `Cargo.toml` nor `lua/` ends the run with an error that names the directory.

`--language lua` analyzes the plugin beside a `Cargo.toml`, and `--language rust` analyzes the Rust workspace whatever the directory holds.
With `--language` set, the run does not look at the directory to choose.

## Namespaces and modules

Every top-level namespace under `lua/` is a package: a directory `lua/<name>/`, or a single file `lua/<name>.lua` for a plugin of one module.
The plugin's repository is the workspace.

Every Lua file in a namespace is a module, named as `require` loads it, with `::` in place of the dot.
`lua/<name>/ui/window.lua` is the module `<name>::ui::window`.
An `init.lua` stands for its directory: `lua/<name>/ui/init.lua` is the module `<name>::ui`, not a child `init`.
Where both `ui.lua` and `ui/init.lua` exist, the module reads `ui.lua`, as `require` does.

## Which `require` calls are edges

A `require` whose only argument is a string literal is an edge, in any spelling: `require("x")`, `require "x"`, `require 'x'` and `require [[x]]`.
A call inside a function is an edge like any other.
A `require` into another namespace of the plugin is also an edge between the two packages.

A `require` of a name that is no module of the plugin makes no edge.
That covers `vim` modules, other plugins and the plugin's own native library.
`--debug` prints each discarded call and their count, for example `discarded 1 of 3 require calls: no module of the plugin`.

The location of an edge is the line of its `require`.
In a page that `cargo arc ui` serves to an editor, a click on the location opens that line.

## Scripts in `plugin/`

The files in `plugin/` are scripts Neovim runs at startup, and they are not modules.
Their `require` calls are edges from the namespace that bears the script's name, the way the imports of a crate's `main.rs` are edges from the crate.
A script whose name matches no namespace belongs to the plugin's only namespace with a root file, `lua/<n>.lua` or `lua/<n>/init.lua`.
With none or several of those, its calls are discarded and counted under `--debug`.

## Tests

The run reads `lua/` and `plugin/` and nothing else in the repository.
Under `lua/` it skips every directory named `spec/` or `tests/` and every file ending in `_spec.lua`.

## Options that apply to Rust only

These options end a Lua run before the analysis, with a message that names each one given:

- `--features`, `--all-features`, `--no-default-features`
- `--hir`
- `--include-tests`
- `--externals`, `--transitive-deps`

`cargo arc check` refuses a Lua plugin too, because the rules apply to Rust only.
`--include-reexports` is accepted and changes nothing, since Lua has no re-exports to filter.

A page that `cargo arc ui` serves for a Lua run has no switch for test code and none for external packages.

## Hotspot map and volatility

`cargo arc hotspots` and `--volatility` count the lines of the `.lua` files and the commits that touched them.
A namespace has no manifest, so the map identifies it by its directory.
In a page that `cargo arc ui` serves, a leaf of the map opens its file in the editor.

## What the run does not see

- A `require` whose argument is not a single string literal: a concatenation, a `gsub`, `...`, a variable, or an alias of `require`.
- Custom path templates in `package.path`. A name resolves only to `lua/<path>.lua` or `lua/<path>/init.lua`.
- Whether a `require` is lazy, guarded by `pcall` or conditional. Every call is the same kind of edge.
- Other plugins and LuaRocks dependencies. They are not nodes, and `--externals` is refused.
- The boundary to a Rust backend. A plugin with `lua/` and a `Cargo.toml` is analyzed as one language per run, and the `require` of the native library is discarded.
- Symbols: the run records which module a `require` loads, not which fields of it the code uses.
- Rules: `check` does not run.
- Test code.
