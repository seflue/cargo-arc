# Rust Coding Standards

Coding standards for this repository. The *Project overlay* at the bottom wins
wherever it contradicts a rule here.

Every rule states its boundary ("fine when ..."). A rule without a boundary is
a preference, and preferences do not belong in a standard.

## Tooling boundary

Rules marked *lint* are machine-checkable. Where the repo configures the named
lints, they are the tool's job and a reviewer skips them; only the rules
without a lint, and the judgement calls inside the lint-covered ones, are worth
a human or agent reading the diff.

Suggested starting point in `Cargo.toml`:

```toml
[lints.clippy]
needless_pass_by_value = "warn"
redundant_closure_for_method_calls = "warn"
unnecessary_wraps = "warn"
manual_let_else = "warn"
uninlined_format_args = "warn"
implicit_clone = "warn"
trivially_copy_pass_by_ref = "warn"
```

## 1. Ownership and allocation

### R1 Panic only where a panic is the correct outcome

`.unwrap()` says "this cannot fail" without saying why, and says it in the one
place the reader most wants a reason. Propagate with `?`, or convert with
`.ok_or_else()`. Where stopping really is right, `.expect()` naming the
violated invariant says so out loud.

The question is not whether the file is a test. It is whether the process
should end here: a test, a build script, a `main` that cannot proceed on a
broken config, an index the surrounding code just proved. In library code
reached from a caller who could have handled it, almost never.

*lint: `clippy::unwrap_used`, `clippy::unnecessary_unwrap`*

### R2 Do not clone to satisfy the borrow checker

`.clone()`, `.to_string()`, `.to_owned()` at a call site usually mean a
lifetime was easier to allocate away than to write. Write the lifetime.

Fine when: the receiver stores the value, the type is small and the clone buys
readability, or it is `Arc::clone` (cheap and intentional).

Red flags: clone inside a loop, `.clone()` on a `Copy` type.

*lint: `clippy::redundant_clone`, `clippy::clone_on_copy`*

### R3 Read-only parameters take `&str`, not `String`

A `String` parameter forces every caller to allocate. Use `impl AsRef<str>` in
a public API where callers vary, `&str` internally.

Fine when: the function stores the value.

*lint: `clippy::needless_pass_by_value`*

### R4 `Arc<Mutex<T>>` is a choice, not a default

Before reaching for it, rule out `mpsc` (producer/consumer), `RwLock`
(read-heavy), `tokio::sync::watch` (broadcast), or message passing.

Fine when: shared state with low contention, where the alternatives add
structure without removing a problem.

## 2. Control flow

### R5 Combinators over ceremonial `match`

`match x { Some(v) => Some(f(v)), None => None }` is `x.map(f)`.

Fine when: arms have side effects, bind several variables, or the combinator
chain reads worse than the match.

*lint: `clippy::manual_map`, `clippy::option_if_let_else`*

### R6 Iterator chains over accumulate-and-push loops

Fine when: the loop has side effects, or uses `break`/`continue` in a way a
chain would obscure. A `for` loop is not worse than `.for_each()`; it is
better.

*lint: `clippy::manual_filter_map`, `clippy::explicit_iter_loop`*

### R7 No redundant closures

`.map(|x| f(x))` is `.map(f)`; `.unwrap_or_else(|| Default::default())` is
`.unwrap_or_default()`.

*lint: `clippy::redundant_closure`*

### N1 The reader should not have to carry conditions

Every enclosing block is a condition the reader holds in their head until the
innermost line, and nesting buries the successful path deepest, where it is
read last. Past two levels beyond the function body, check whether that is
still true here; the count is the symptom, not the rule.

Flatten with early return, `?`, `let-else`, or an extracted helper that names
the sub-operation.

```rust
// <example>.rs
let Some(s) = input else {
    return Err(Error::NoInput);
};
let parsed = parse(s).map_err(|_| Error::ParseFailed)?;
if !parsed.is_valid() {
    return Err(Error::Invalid);
}
Ok(transform(parsed))
```

Fine when: a `match` over an enum whose arms are one to three lines, or parser
code whose nesting mirrors the grammar.

### N2 One level of abstraction per function

Length is a symptom, not the rule. A function is too long when it mixes
orchestration with detail, when blank-line-separated blocks do unrelated work,
or when comments act as section headers inside it.

Fine when: the whole body sits at one level, however long (a wide `match` with
simple arms, a sequential pipeline that would lose its thread if split).

## 3. Types

Make illegal states unrepresentable, and prefer the compiler to a convention.

### T1 No boolean parameters whose meaning dies at the call site

`widget.paint(true, false)` says nothing. Two-variant enums cost nothing and
read at the call site.

Fine when: a single self-evident flag (`is_recursive: bool`), a private helper
whose caller is adjacent, or a builder setter.

### T2 Newtypes for repeated primitives

Three `u64` parameters in a row are three chances to swap two of them.
`UserId(u64)`, `ProductId(u64)`, `Quantity(u64)` make the swap a compile error.

Fine when: a single ID parameter, or a hot inner loop where the wrapper is in
the way.

### T3 Enum states instead of flag-plus-`Option` structs

A struct carrying `is_connected: bool` next to `socket: Option<TcpStream>`
encodes a rule that nothing enforces.

```rust
// <example>.rs
enum Connection {
    Disconnected,
    Connected { socket: TcpStream },
    Authenticated { socket: TcpStream, token: String },
}
```

Fine when: the flags genuinely compose independently, so two bools really do
mean four valid states.

### G1 Generics by default, `dyn` when you need type erasure

`&impl Handler` over `&dyn Handler`; `-> impl Parser` over `-> Box<dyn Parser>`,
or just return the concrete type when there is one implementation.

Fine when: heterogeneous collections (`Vec<Box<dyn Step>>`), plugin
boundaries, or cutting monomorphization out of compile times.

### G2 No `Deref` on a domain newtype

`impl Deref for EmailAddress { type Target = String }` makes
`email.push_str("junk")` compile. Expose what the type means: an inherent
`as_str`, a `Display` impl.

Fine when: the wrapper is semantically a smart pointer, or wraps `str`/`Path`
for coercion.

### G3 Extension trait when you only add behaviour

Need a distinct type for trait impls, type safety, or an API boundary? Newtype.
Only adding methods to a foreign type? Extension trait.

## 4. Errors

### E1 Context at every abstraction boundary

Bare `?` on I/O or parsing loses which file, which field, which step.

```rust
// <example>.rs
let text = std::fs::read_to_string(path)
    .with_context(|| format!("failed to read config from {}", path.display()))?;
```

Fine when: an internal helper whose caller adds the context, or a function
short enough that the source is unambiguous.

### E2 `Option` means absent, `Result` means failed

`fn find_user(id) -> Option<User>` cannot distinguish "no such user" from "the
database is down". Return `Result<Option<User>, DbError>`.

Fine when: lookup in an in-memory collection, or an optional field, where
absence is the only failure mode.

### E3 Typed errors where a caller branches, opaque errors elsewhere

The question is not whether the crate is a library. It is whether any caller
has to tell one failure from another: retry on a rate limit, fall back when a
file is missing, show a validation message. Where that happens, the error is an
enum with `#[error(...)]` variants and `#[from]` conversions (`thiserror`).
Where nothing branches and the error only travels upward to be printed, an
opaque `anyhow::Error` carrying `.context()` is less to maintain.

The two sides are not symmetric, which is the part worth remembering.
`thiserror` in application code costs a derive. `anyhow` in a published
signature is a one-way door: callers can never match on it, and handing them
variants later is a breaking change. Undecided at a public boundary, take the
typed side.

Fine when: tests, internal helpers whose caller supplies the context, and
modules whose error taxonomy has not settled. Convert at the boundary that
publishes them.

## 5. Visibility and layout

### R9 `pub` is a promise

`pub` is the public API and removing it is a breaking change. `pub(crate)` for
cross-module sharing, `pub(super)` for a parent, private by default. Export
through deliberate re-exports in `lib.rs` rather than `pub mod` on everything.

Red flag: a new `pub fn` in an internal module with no caller outside it.

### R10 Enums instead of stringly-typed values

A `String` with a known set of values is an enum that has not been written yet.
Comparing against `"warning"` is a typo away from silently never matching.

### Module files

Prefer `parser.rs` beside `parser/`, not `parser/mod.rs`. Keep module roots
thin: declarations and re-exports, implementation in submodules.

Fine when: the codebase already uses `mod.rs`. Consistency beats the
convention.

## 6. Style

### S1 Every `unsafe` block states its invariant

The compiler stopped checking, so the reason the invariant holds has nowhere
else to live. An `unsafe` block without a `// SAFETY:` comment naming why it
holds is a finding.

```rust
// <example>.rs
// SAFETY: `idx < self.len` was checked by the caller above, and `self.buf` is
// never reallocated while `&self` is held.
unsafe { self.buf.get_unchecked(idx) }
```

*lint: `clippy::undocumented_unsafe_blocks`*

### S2 No abstraction for one implementation

A trait plus a single implementing struct is indirection with no second caller.
Write the function.

Fine when: a second implementation exists or is concretely planned, or the
trait is the public contract.

### S3 Validate at boundaries, trust invariants inside

Re-checking something the parser already guaranteed adds an error path that
cannot be reached and cannot be tested. Validate user input, external APIs, and
deserialization; nothing behind them.

### S4 Async hygiene

`tokio::sync::Mutex` is for holding a lock across an `.await`. Without one,
`std::sync::Mutex` is correct and cheaper. Sequential `.await` in a loop over
independent work wants `join_all`.

## 7. Tests

A panicking test is a failing test, which is why R1 rarely bites here.

### X1 Parameterize near-identical tests

Three or more tests differing only in input and expectation are one `rstest` or
`test-case` table.

Fine when: each case needs its own setup, or the test names carry documentation
that a table would erase.

### X2 Fixtures for repeated construction

The same struct literal in five tests is a `make_admin(name)` helper or an
`#[fixture]`.

### X3 Assertions that show the difference

`pretty_assertions` for any `assert_eq!` on structs or strings. `insta` for
complex serialized output. `proptest` for roundtrip and invariant properties.

## Dependencies

A dependency earns its place when it removes a class of bug or a body of
hand-maintained boilerplate. The test: would you write the macro yourself if
the crate did not exist?

| Crate | Earns its place when | Does not |
|---|---|---|
| `thiserror` | manual `Display` + `Error` impls on an error enum | one variant |
| `anyhow` | errors that only travel upward to be printed | a signature a caller branches on (see E3) |
| `derive_more` | several manual `From`/`Display`/`Into` impls | a single derive |
| `strum` | hand-written enum/string conversion tables | two variants |
| `typed-builder` | constructors with four or more required fields | two or three fields |
| `itertools` | chunks, windows, `join`, `sorted`, `unique` | plain filter/map/collect |

Never: a crate for a problem the repo does not have, two crates for one
problem, or a heavy dependency tree bought for convenience.

## Project overlay

Everything below overrides the baseline. Delete the rows that do not apply.

| Aspect | This repo |
|---|---|
| Error handling | `anyhow` on the application path (`cli.rs`, `analyze/*`). `thiserror` only where a typed error crosses an API boundary — currently `volatility.rs` alone. |
| Crate structure | One crate, `lib.rs` plus `main.rs`, modules by concern (`analyze`, `diagnose`, `layout`, `render`, `rules`). The `ra_ap_*` backend sits behind the optional `hir` feature. |
| Concurrency | None on the analysis and request path, no async runtime. `ui/server.rs` alone starts threads (see the deviation below). |
| Build and test commands | `just test` (cargo test + bun test), `just lint`, `just fmt`. |
| Lint configuration | `[lints.clippy] pedantic = "warn"`; CI runs `cargo clippy --all-targets -- -D warnings`. |

### Deviations from the baseline

State the rule, the deviation, and why. A deviation without a reason will be
re-litigated by the next reader.

**Threads in `ui/server.rs`.** The overlay rules out concurrency; the jump
service's transport uses `std::thread::scope` for the editor's stdin reader and
for each open event stream. The request loop stays sequential on the calling
thread. A blocking `tiny_http` request loop cannot also read stdin or hold a
stream open, and an async runtime for two blocking readers would be the larger
dependency. The module is the only one in the crate that starts a thread.

**A lock in `ui/service.rs`.** The service holds the editor's colour mode
behind a `std::sync::Mutex`, written by the stdin thread and read when a page
is served. It is the one piece of shared state on the request path: a page
loaded after the editor's line has to start in that mode, and the value is a
two-variant enum with no contention worth a channel.
