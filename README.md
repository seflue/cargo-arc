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
cargo-arc

# Specific workspace, write to file
cargo-arc -m /path/to/Cargo.toml -o deps.svg
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
