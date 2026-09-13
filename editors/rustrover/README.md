# cargo-arc for RustRover

Starts `cargo-arc arc ui` for the project's working directory and shows the
diagram in a tool window, jumping to the file and line the service names when
a jump target is clicked. The service is a child of the project and ends with
it when the project closes.

## Requirements

RustRover 2025.1 or newer with the bundled JetBrains Runtime (JCEF comes with
it), JDK 21 or newer to build (the JBR inside the RustRover install works:
point `JAVA_HOME` at `<install>/jbr`), and a `cargo-arc` binary that has the
`ui` subcommand.

## Install

Not distributed yet. Build and install it from this checkout:

1. `cargo build --release` in the repository root.
2. `cd editors/rustrover && ./gradlew buildPlugin`. The first run downloads
   Gradle and a RustRover (about 1 GB) into Gradle's cache; the zip lands in
   `build/distributions/`. To build against the RustRover you already have
   instead, set `rustRoverPath` as described under "Use your installed
   RustRover" before this step.
3. In RustRover: Settings → Plugins → gear icon → Install Plugin from Disk →
   that zip → restart.
4. Settings → Tools → cargo-arc → binary path to
   `<checkout>/target/release/cargo-arc`.
5. Open a Rust project, then Tools → cargo-arc: Open (or the tool window on
   the right).

## Use your installed RustRover

Every Gradle task compiles against a RustRover, downloaded by default. The
Gradle property `rustRoverPath` points it at an installed one instead, and
nothing is downloaded. Set it once in `~/.gradle/gradle.properties`:

```
rustRoverPath=/path/to/rustrover
```

or per command with `-PrustRoverPath=/path/to/rustrover`. On a Toolbox
install the path is the app directory, for example
`~/.local/share/JetBrains/Toolbox/apps/rustrover`.

## Run a sandbox IDE

`./gradlew runIde` runs the plugin in a second RustRover instance with its
own sandbox instead of installing the zip. With `rustRoverPath` set, that
instance is your installed RustRover.

## Actions

- `cargo-arc: Open` shows the tool window and starts the service, or shows
  its page again if the service already runs.
- `cargo-arc: Restart` ends the service and starts a new one; the tool
  window's page reloads by itself once the new service announces its port.
- `cargo-arc: Stop` ends the service.

Restart and stop are also title actions of the tool window, and disabled
while no service runs.

## Settings

Settings → Tools → cargo-arc, all optional:

| Option          | Scope       | Default       | Passed as                 |
| --------------- | ----------- | ------------- | -------------------------- |
| `binary`        | Application | `cargo-arc`   | the program to run         |
| `manifest_path` | Project     | none          | `--manifest-path`          |
| `features`      | Project     | none          | `--features a,b`           |
| `include_tests` | Project     | off           | `--include-tests`          |
| `externals`     | Project     | off           | `--externals`              |

A changed setting takes effect on the next restart.

## Tests

`just test-rustrover` from the repository root. It runs `./gradlew test` in
`editors/rustrover`, JUnit only, no platform test framework.
