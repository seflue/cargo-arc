# cargo-arc for VS Code

Starts `<binary> arc ui` for the workspace folder and shows the diagram in a webview panel beside the editor.
Clicking a jump target in the diagram opens the file at that line.
The service is a child of this VS Code window and ends when the panel closes, when it is stopped, or with the window.

Requires VS Code 1.80 or newer and a `cargo-arc` binary that has the `ui` subcommand.

## Run the development version

Not distributed yet.
Run it from this checkout.

Build the binary in the repository root with `just build --release` (or `cargo build --release`).
Open the folder `editors/vscode` itself in VS Code (not the repository root; the launch configuration lives in its `.vscode/`).
Start the launch configuration "Run Extension": F5 if it is bound to "Debug: Start Debugging", otherwise "Debug: Select and Start Debugging" from the command palette, or the Run and Debug view.
A second window, the Extension Development Host, opens with this repository as its workspace, and `cargoArc.binary` already points at `target/release/cargo-arc` (from `dev.code-workspace`).
The `cargo-arc` commands exist only in that second window.
Run "cargo-arc: Open Diagram" from its command palette.

To run against another project instead, use `code --extensionDevelopmentPath=/path/to/cargo-arc/editors/vscode /path/to/project`.
Then set `cargoArc.binary` in that window's settings to the absolute path of the release build, or install the binary and leave the default.

## Commands

- `cargo-arc: Open Diagram` starts the service and opens the panel. While the service runs, it reveals the panel again instead of starting a second one.
- `cargo-arc: Open Diagram in Browser` opens the same page in the system browser, for a second screen. It starts the service if none runs; the panel is not needed.
- `cargo-arc: Restart Service` ends the service and starts a new one on the same port. Once the new service announces it is ready, the diagram in the panel reloads automatically; a browser tab needs a manual reload.
- `cargo-arc: Stop Service` ends the service.

## Settings

| Setting                 | Default     | Passed as                                                                                            |
| ------------------------ | ----------- | ----------------------------------------------------------------------------------------------------- |
| `cargoArc.binary`        | `cargo-arc` | the program, from `PATH`; a value containing a path separator is resolved against the workspace folder, after `~` and `$VAR` are expanded |
| `cargoArc.manifestPath`  | `""`        | `--manifest-path`                                                                                      |
| `cargoArc.features`      | `[]`        | `--features a,b`                                                                                       |
| `cargoArc.includeTests`  | `false`     | `--include-tests`                                                                                      |
| `cargoArc.externals`     | `false`     | `--externals`                                                                                          |

## Keyboard

With the panel focused, all keys go to the diagram page.
Tab is the only way to move focus out of the panel.

## Tests

`bun test editors/vscode` from the repository root, or `just test-js`.
