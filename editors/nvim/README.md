# cargo-arc for Neovim

Starts `cargo-arc arc ui` for Neovim's working directory, opens the diagram in
the browser, and jumps to the file and line the service names when a jump
target is clicked. The service is a child of this Neovim session and ends with
it.

Requires Neovim 0.12 and a `cargo-arc` binary that has the `ui` subcommand.

## Install

Not distributed yet. Load it from this checkout, for example with lazy.nvim:

```lua
{
  dir = '/path/to/cargo-arc/editors/nvim',
  opts = {},
}
```

For development against this repository, point at the release build:

```lua
{
  dir = '/path/to/cargo-arc/editors/nvim',
  opts = { binary = '/path/to/cargo-arc/target/release/cargo-arc' },
}
```

## Commands

- `:Arc open` starts the service and opens the page. While the service runs,
  it opens the page again.
- `:Arc restart` ends the service and starts a new one on the same port
  (`arc ui --port`), then notifies you. Reload the open page to see the code
  as it is now; no new tab is opened.
- `:Arc stop` ends the service.

## Options

`require('cargo-arc').setup({ ... })`, all optional:

| Option          | Default       | Passed as                |
| --------------- | ------------- | ------------------------ |
| `binary`        | `'cargo-arc'` | the program, from `PATH` |
| `manifest_path` | `nil`         | `--manifest-path`        |
| `features`      | `{}`          | `--features a,b`         |
| `include_tests` | `false`       | `--include-tests`        |
| `externals`     | `false`       | `--externals`            |

## Tests

`just test-nvim` from the repository root. It clones mini.nvim into `deps/`
on first use, builds the release binary, and runs `tests/` in headless child
Neovim instances. One case runs the real service against
`tests/fixtures/multi_crate`; the others use `tests/fake_service.lua`.
