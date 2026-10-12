# ADR-030: One Language per Run, and a Lua Adapter

- **Status:** Active
- **Decided:** 2026-10-12
- **Related:** ADR-013 (syn as the Rust backend), ADR-029 (language adapter)

## Context

ADR-029 put the analysis behind a language adapter and a language profile, with Rust as the only adapter.
The second language is Lua, for Neovim plugins.
A Neovim plugin keeps its modules under `lua/` and has no manifest that lists them.

Some repositories hold both: a Cargo workspace and a Lua plugin beside it, cargo-arc's own among them.
Every existing invocation analyzes Rust, and many options name Cargo concepts: features, test code, external crates, rust-analyzer HIR.
The rules of `check` are written against Rust module paths.

## Decision

A run analyzes one language, and the language is chosen before the analysis starts.
Exactly one adapter runs.

cargo-arc detects the language from the directory it is pointed at.
A Cargo manifest means Rust, also where a `lua/` directory lies beside it.
A `lua/` directory without a Cargo manifest means Lua.
A directory with neither is an error.
An option names the language explicitly, and when it is given, the detection does not run.

A Lua run refuses the options that have a meaning only for Rust, and refuses `check`.
The refusal comes before the analysis and names each refused option.
An option that has nothing to act on in Lua but cannot mislead is accepted.

The Lua adapter parses with full_moon, as Lua 5.1.

## Rationale

- One language per run keeps the graph, the layout and the page unchanged. A mixed graph would need a way to show two languages in one diagram, and there is none yet.
- Rust winning the detection keeps every existing invocation where it was, including in repositories that hold both.
- A refused option tells the user it had no effect. A silently ignored one leaves them reading a diagram they believe the option shaped.
- full_moon parses Lua 5.1, the dialect of LuaJIT that Neovim embeds, and keeps the line of each call.

## Consequences

- Rust users see no change.
- A repository with both languages needs two runs to show both, and no run shows an edge from Lua into the Rust backend.
- Each new language adds a detection rule and decides which options it refuses.
- An option that later gains a meaning for Lua moves from refused to accepted.
- The Lua adapter answers the questions ADR-013 answers for Rust with full_moon instead of syn.

## References

- `src/cli.rs`: `language_of`, the detection and the override, and `refuse_rust_only_flags`.
- `src/analyze/lua.rs`: the Lua adapter and its profile.
- `docs/LUA_PLUGINS.md`: what a Lua run analyzes and what it refuses.
