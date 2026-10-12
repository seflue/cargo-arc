# ADR-026: Editor Plugins Build Their cargo-arc CLI

- **Status:** Active
- **Decided:** 2026-10-08
- **Related:** ADR-027 (plugins run `cargo arc ui`)

## Context

Under ADR-027 a plugin starts `cargo arc ui` and reads its stdout.
The plugin looks for the cargo-arc binary at a configured path or on `PATH`.
The user installs the binary from crates.io, and it changes only when the user installs it again.

The VS Code and RustRover plugins are meant for their marketplaces, and both marketplaces update plugins automatically.
Neovim plugin managers such as lazy.nvim clone the plugin's repository and run the plugin's build step after every install and update.

crates.io ships source code, not binaries, and the GitHub releases carry no binaries.
cargo-arc reads the workspace through cargo, so every user has cargo.

## Decision

A plugin builds the cargo-arc release it was published with from crates.io into a directory of its own.
Each plugin carries the crate's version number.

A plugin looks for the binary in this order: the path the user configured, the binary the plugin built, `PATH`.

The first build waits for the user's consent.
After that, a plugin update builds the matching release without asking, and a setting makes it ask instead.

The Neovim plugin builds in the plugin manager's build step, and builds the release whose version its checked-out source carries.
Its version follows the tag the plugin manager checks out.
Adding the plugin to the configuration counts as consent.

## State per plugin

- The Neovim plugin builds its CLI as decided.
- The VS Code plugin does not build yet. It runs the configured binary or `cargo-arc` from `PATH`, and its version is 0.0.0.
- The RustRover plugin does not build yet. It runs the configured binary, which defaults to `cargo-arc` from `PATH`.

## Rationale

- A marketplace update reaches the user without any action. A CLI tied to the plugin arrives with it.
- Building from source needs no cross-compiling and no release artifacts per platform. It works wherever cargo-arc compiles.
- Releases come days to months apart, so a user builds rarely.
- Every CLI release needs a plugin release that delivers it. Separate version numbers would need a table of which plugin version belongs to which CLI.
- A configured path comes first, so a user with an installation of their own keeps it.
- The Neovim plugin builds the crates.io release and not its checkout, because building the checkout needs more tools than cargo.

## Consequences

- The first install and every update cost compile time on the user's machine. During the build the plugin keeps working with the old binary.
- A fix to a plugin alone needs a crate patch release without a code change, and every CLI release publishes every plugin again.
- A failed build shows cargo's output. A Rust version that is too old gives a readable error as long as the crate declares its minimum Rust version, which it does.
- The lookup order and the name of the update setting end up in user configurations and are fixed after the first release. How the binary reaches the plugin's directory can still change, for example to prebuilt binaries.
- On a platform where cargo-arc compiles but was never tested, the user gets an untested binary.
- The Neovim plugin loads from the repository root, so the plugin specification a user copies names only the repository.

## References

- `build.lua`: the Neovim plugin's build step.
- `editors/nvim/lua/cargo-arc/init.lua`: `binary`, the lookup order, and `install_root`.
- `editors/vscode/service.js`, `editors/rustrover/src/`: binary lookup in the other two plugins.
