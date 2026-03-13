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
    npx tsc --project jsconfig.json
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

# idempotent release: bump version + changelog first, then run this
release:
    ./scripts/release.sh

clean:
    cargo clean
