# cargo-arc

default:
    @just --list

build:
    cargo build

test-rust:
    cargo test

test-js:
    bun test

# Rust + JS
test: test-rust test-js

# clippy + biome + tsc typecheck + format check + cycle detection
lint:
    cargo clippy -- -D warnings
    cargo fmt --check
    bunx biome check js/
    bunx tsc --project jsconfig.json
    cargo run -- arc --check

# format Rust + JS
fmt:
    cargo fmt
    bunx biome format --write js/

# auto-fix lint warnings
fix:
    cargo clippy --fix --allow-dirty
    bunx biome check --write js/

diagram:
    cargo run -- arc

install:
    cargo install --path .

# show what a release would change, without touching anything
release-dry version:
    cargo release {{version}}

# release: collect changelog bullets under Unreleased first, then run this
release version:
    ./scripts/release.sh {{version}}

clean:
    cargo clean
