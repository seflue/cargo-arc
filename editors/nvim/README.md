# cargo-arc for Neovim

Starts `cargo-arc arc ui` for Neovim's working directory, opens the diagram in
the browser, and jumps to the file and line the service names when a jump
target is clicked. In the other direction, the diagram follows the editor:
entering a buffer selects that file's node in the page, as a click would, and
writing a file recomputes the diagram. The service is a child of this Neovim
session and ends with it.

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
- `:Arc follow off` stops the page from following the editor; `:Arc follow
  on` resumes it. The page's own "Follow editor" button does the same.
- `:Arc externals on|off` and `:Arc tests on|off` take external crates or
  test code into the analysis or leave them out. The service recomputes in
  place and the page reloads with its view kept; the page's "External
  crates" and "Test code" buttons do the same. A notice names the state
  reached, or the error if the analysis failed. `:Arc restart` starts the
  new service with the state last reached, not with the options below;
  `:Arc stop` and `:Arc open` start from the options again.
- `:Arc on-save off` stops a written file from recomputing the diagram;
  `:Arc on-save on` resumes it. The page's "Recompute on save" button does the
  same. The plugin reports every write; the service recomputes for `.rs` files
  and `Cargo.toml` under the workspace root. A file that does not parse keeps
  the diagram as it was, and a notice names the file.
- `:Arc recompute` runs the analysis once with the current state.

## Options

`require('cargo-arc').setup({ ... })`, all optional:

| Option          | Default       | Passed as                |
| --------------- | ------------- | ------------------------ |
| `binary`        | `'cargo-arc'` | the program, from `PATH` |
| `manifest_path` | `nil`         | `--manifest-path`        |
| `features`      | `{}`          | `--features a,b`         |
| `include_tests` | `false`       | `--include-tests`        |
| `externals`     | `false`       | `--externals`            |

`include_tests` and `externals` set the state a service starts with; `:Arc
externals` and `:Arc tests` change it while the service runs, and `:Arc
restart` carries it over.

## Tests

`just test-nvim` from the repository root. It clones mini.nvim into `deps/`
on first use, builds the release binary, and runs `tests/` in headless child
Neovim instances. One case runs the real service against
`tests/fixtures/multi_crate`; the others use `tests/fake_service.lua`.
