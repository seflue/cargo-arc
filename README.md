# cargo-arc

Workspace architecture visualization for Rust projects.
Generates SVG diagrams showing module hierarchies and cross-crate dependencies.

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Analyze current workspace
cargo arc

# Specific workspace, write to file
cargo arc -m /path/to/Cargo.toml -o deps.svg
```

### Feature Filtering

When using `--features`, only crates that define the specified feature (seeds) and their dependencies are shown.

**Important:** Use `--no-default-features` to exclude default dependencies:

```bash
# Show only crates involved in the "web" feature (includes default deps)
cargo arc --features web -o web-deps.svg

# Show ONLY the "web" feature graph (excludes default deps)
cargo arc --features web --no-default-features -o web-deps.svg

# Debug filtering decisions
cargo arc --features web --no-default-features --debug 2>debug.log -o web-deps.svg

# Alternative: use RUST_LOG for fine-grained control
RUST_LOG=cargo_arc=debug cargo arc -o deps.svg 2>debug.log
```

## Development

Requires [Just](https://github.com/casey/just) as task runner.

```bash
just build
just test    # Rust + JS
just lint    # clippy + format check
just fmt
```

## License

MIT OR Apache-2.0
