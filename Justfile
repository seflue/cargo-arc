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

# clippy + format check
lint:
    cargo clippy -- -D warnings
    cargo fmt --check

fmt:
    cargo fmt

diagram:
    cargo run -- arc

clean:
    cargo clean
