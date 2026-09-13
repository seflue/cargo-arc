# cargo-arc

default:
    @just --list

# extra arguments go to cargo: `just build --release`
build *args:
    cargo build {{ args }}

test-rust:
    cargo test

test-js:
    bun test

# Rust + JS
test: test-rust test-js

nvim_dir := "editors/nvim"

# Neovim plugin: fetches mini.nvim into deps/ once, needs the release binary
test-nvim:
    test -d {{nvim_dir}}/deps/mini.nvim || git clone --depth 1 https://github.com/echasnovski/mini.nvim {{nvim_dir}}/deps/mini.nvim
    cargo build --release
    cd {{nvim_dir}} && nvim --headless --noplugin -u scripts/minimal_init.lua -c "lua MiniTest.run()"

rustrover_dir := "editors/rustrover"

# RustRover plugin: JUnit tests, no platform test framework
test-rustrover:
    cd {{rustrover_dir}} && ./gradlew test

# clippy + biome + tsc typecheck + format check + cycle detection
lint:
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check
    bunx biome check js/
    bunx tsc --project jsconfig.json
    cargo run -- arc check

# format Rust + JS
fmt:
    cargo fmt
    bunx biome format --write js/

# auto-fix lint warnings
fix:
    cargo clippy --fix --all-targets --allow-dirty
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
