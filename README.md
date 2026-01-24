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
```

## Development

```bash
cargo test
cargo clippy
cargo fmt
```

## Testing

```bash
# Fast unit tests (<1s)
cargo test

# Slow smoke tests only (~60s, requires rust-analyzer)
cargo test -- --ignored

# All tests
cargo test -- --include-ignored
```

## License

MIT OR Apache-2.0
