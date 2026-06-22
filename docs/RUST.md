# Rust conventions

How we write Rust in joblode. This is the detail behind CLAUDE.md's one-liner ("rustfmt-clean,
clippy-warning-free"); read it before non-trivial Rust work. It is **project-tailored**, not a generic
style essay — when it and the [Rust API Guidelines][api] disagree about something general, the API
Guidelines win; when they disagree about *this codebase*, this file wins.

Sources: [Rust API Guidelines][api] · [Rust Style Guide][style] · [Clippy lint groups][clippy] ·
the [thiserror-vs-anyhow][err] rule of thumb.

[api]: https://rust-lang.github.io/api-guidelines/checklist.html
[style]: https://doc.rust-lang.org/style-guide/
[clippy]: https://doc.rust-lang.org/stable/clippy/lints.html
[err]: https://momori.dev/posts/rust-error-handling-thiserror-anyhow/

## Gates (non-negotiable — CI enforces them)

- `cargo fmt --all` clean. Don't hand-format; let rustfmt decide. Never `#[rustfmt::skip]` to win an argument.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean. **Zero warnings**, including
  in tests. Fix the cause, don't silence it.
- `cargo test --workspace` green. New behavior lands with a test (TDD — see DESIGN §8).
- `cargo deny check` clean. New deps must pass advisories / licenses / bans / sources.

## Lints & `#[allow]`

- Treat a warning as a bug. The first move is to fix the code, not to allow the lint.
- When a lint is a genuine false positive, scope the `#[allow]` as tightly as possible (the item, not the
  module/crate) and **always give a reason**: `#[allow(clippy::x, reason = "…")]`. A reason-less `#[allow]`
  will be questioned in review.
- Prefer a workspace `[lints]` table over `#![allow]`/`#![warn]` sprinkled across files, if we adopt a
  crate-wide policy. Library crates may opt into `clippy::pedantic` — expect to pair it with reasoned
  allows.

## Naming & API shape

- Casing per RFC 430 (`CamelCase` types, `snake_case` fns/vars, `SCREAMING_SNAKE_CASE` consts). Keep word
  order consistent across related names.
- Conversions follow `as_` (cheap borrow), `to_` (expensive/owned), `into_` (consuming). Getters are
  `field()` / `field_mut()`, not `get_field()`.
- Derive the obvious traits on public types: `Debug` always; then `Clone`, `PartialEq`, `Eq`, `Hash`,
  `Default` where they make sense. Implement `From`/`AsRef` for natural conversions rather than ad-hoc
  methods.
- When a type wraps a non-`Debug` resource (a DuckDB `Connection`, an rmcp `ToolRouter`), still provide
  `Debug` via a hand-written impl that shows the useful fields and `.finish_non_exhaustive()` — see
  `JobStore` / `JobServer`. Don't drop `Debug` just because one field can't derive it.
- Keep struct fields private unless the type is a plain data record (our `Job`/`Criteria` are intentionally
  public-field DTOs). Public fields are an API promise.
- Accept the most general type you can (`impl AsRef<Path>`, `&str`, `impl IntoIterator`); return concrete
  types. Borrow in arguments (`&T`, `&str`, `&[T]`) instead of taking ownership you don't need.

## Errors

- **Libraries (`joblode-core`, future `joblode-rank`): typed errors.** Prefer a `thiserror` enum so callers
  can match variants, over leaking a dependency's error type. (Core currently surfaces `duckdb::Error`; give
  it its own error enum as it grows beyond a thin wrapper.)
- **Binaries (`joblode-server`): `anyhow`.** Use `?` freely; add context with `.context("opening parquet")`
  at the point of failure. Map foreign errors to a clear message at the boundary.
- Error messages are lowercase, no trailing period, and describe *this* failure only — don't re-format the
  source error in your text; let the error chain carry it.
- Document every `Result`-returning public fn with an `# Errors` section, and every fn that can panic with a
  `# Panics` section.

## Panics & unwrap

- No `unwrap()`/`expect()` on genuinely fallible paths in library or request-handling code — return a
  `Result`. `panic!` is for broken invariants, not for bad input.
- `expect("reason")` is allowed only for true invariants that cannot fail given the code around them, and the
  string states *why* it can't (e.g. `"store mutex poisoned"`). It reads as an assertion, not error handling.
- `unwrap()` is fine in tests and in `#[cfg(test)]` setup.
- Validate arguments at public boundaries; fail with a typed/`anyhow` error, not a panic, on caller mistakes.

## Async & blocking (we run on tokio)

- DuckDB calls are **blocking**. Run them inside `tokio::task::spawn_blocking` so a query never stalls the
  async runtime — this is the established pattern in `joblode-server::mcp`.
- Never hold a `std::sync::Mutex` guard across an `.await`. If a lock must span async work, use
  `tokio::sync::Mutex`; otherwise lock, do the sync work, drop the guard (inside `spawn_blocking` for us).
- Handlers shared across requests must be `Send + Sync + 'static`. A non-`Sync` resource (like a DuckDB
  `Connection`) goes behind `Arc<Mutex<…>>`.
- Don't `.await` while holding more than you need; clone the `Arc` and move it into the blocking task.

## Modules, types, dependencies

- Prefer newtypes over bare `bool`/`String`/`usize` when a value carries meaning that can be confused.
- Use builders for many-optional-field construction; `#[serde(default)]` for optional wire fields.
- Add dependencies deliberately: minimal feature sets (turn off `default-features` when you only need a
  slice), permissive licenses, and justify anything heavy. One job per crate (DESIGN §2: "one core, many
  faces").
- `use` imports grouped std / external / crate-local (rustfmt orders within groups). Avoid glob imports
  except a crate's blessed `prelude`/`model::*`.

## Documentation

- Every public item gets a rustdoc comment (`///`); every crate gets a `//!` header describing its role.
- Doc comments say *what and why*, in full sentences. Code examples in docs should compile (they run under
  `cargo test`).
- Comments explain intent, not mechanics. Don't narrate what the code obviously does.

## Testing

- Contract and failing test first, then implementation (TDD). Keep `main` green.
- Data tests use the committed `testdata/fixture.parquet`; model calls are mocked behind a trait. No network
  or live dataset in unit tests.
- Name tests as behavior (`treats_us_remote_scopes_as_us_jobs`), not mechanics (`test_search_2`). Assert on
  observable output (ids, totals, schema), not internals.
- Prefer deterministic logic (parsing, validation, sorting) we can test over prompting a model to "be
  careful."
