# CLAUDE.md

Behavioral guidelines to reduce common LLM coding mistakes, plus joblode-specific context. Keep this file
**lean** — it loads into every session. Architecture, decisions, and the phased plan live in
[docs/DESIGN.md](docs/DESIGN.md); read it before non-trivial work, and don't duplicate it here.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:

- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:

- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:

- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:

- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:

```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

## The project

**joblode** — an MCP-native job-search service over the open-jobs dataset (~1M live roles). A Rust backend
(DuckDB + axum + `rmcp`) and a React/TS frontend, designed to be driven by an agent. Two languages only;
**there is no Python in this repo.**

Map:

- `crates/joblode-core` — search / get / `semantic_search` over DuckDB (cosine over embeddings) + `embeddings()`. Most data behavior lives here.
- `crates/joblode-rank` — ranking: keyless taste ranker (Rocchio over embeddings, learned from feedback) + optional cheap-model `match`/`pairwise` refinement behind a `ModelClient` trait; plus `EmbedClient` (query embedding) for semantic search.
- `crates/joblode-server` — axum: REST + SSE + MCP (stdio & HTTP) + the MCP App `ui://` resource.
- `web/` — React (Vite, TS): the web UI and the MCP App resource, from one build.
- `docs/DESIGN.md` — source of truth for architecture and the phase plan.

## Golden rules

- **TDD.** New behavior starts with a failing test, then the implementation. Keep `main` green.
- **Stateless server.** We own reads + rendering; durable user state (shortlist, application status)
  belongs to the agent (a spreadsheet + memory). Don't add a persistence/write layer without revisiting
  DESIGN §13.
- **Small PRs**, each mapping to a phase/task in DESIGN. Conventional-commit messages (`feat:`, `fix:`…).
- **Never commit** data (`*.parquet`), secrets, or `resume.txt`. The server binds to `127.0.0.1`.
- Structured job fields are LLM extractions — confirm comp / work-authorization / location against
  `jd_markdown` before relying on them. The `url` is the only apply link; never invent roles.

## Commands

Toolchain is pinned by flox — run `flox activate` first (provides cargo, node, pnpm, duckdb, cargo-deny,
cargo-llvm-cov).

```bash
# Rust
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all
cargo deny check

# Web (from repo root)
pnpm install
pnpm turbo run lint typecheck test build
```

CI gates on all of the above (plus coverage). Run them before pushing.

## Conventions

- **Rust style & idioms: [docs/RUST.md](docs/RUST.md)** — read before non-trivial Rust work.
- **TS/React style & idioms: [docs/FRONTEND.md](docs/FRONTEND.md)** — read before non-trivial frontend work.
- Rust: edition 2021, rustfmt-clean, clippy warning-free.
- TS: strict `tsconfig`, ESLint flat config; keep the frontend minimal.
- Prefer deterministic code (parsing, validation, sorting) over asking the model to "be careful."

## Logs

Turbo writes one rolling log per task per package at `<package>/.turbo/turbo-<task>.log` (e.g.
`web/.turbo/turbo-dev.log` for the Vite dev server) — read these directly when the web app misbehaves.
For the Rust server, read its stdout/stderr or run `cargo run -p joblode-server`.
