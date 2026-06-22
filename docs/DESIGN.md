# Design: open-jobs as an MCP-native, agent-orchestrated job search

Status: **implementation in progress — Phases 0–1 complete** · Owner: Conglei · Last updated: 2026-06-22

This document is the architecture and phased implementation plan. Resolved decisions are recorded in
§11; completed phases are marked in §8.

---

## 1. Vision

Turn the open-jobs dataset into a service that an agent (Claude) drives end to end. The human supplies a
resume and intent; **Claude** runs the hunt:

1. **Narrow criteria** — Claude talks with the user and converges on hard filters (function, level, city,
   comp floor, work authorization, …).
2. **Search** — Claude calls our **MCP server's `search` tool** to draw the candidate set from ~1M live roles.
3. **Rank (token-aware)** — if the user configured a cheap model key (e.g. Gemini Flash-Lite), the server
   ranks the candidates *itself* and returns only a scored shortlist, so Claude never has to read 50 full
   JDs to sort them.
4. **Render** — results come back as an **MCP App** (the official MCP UI extension, supported by Claude
   since 2026-01-26) so Claude shows a real, interactive React table instead of a text dump.
5. **Warm intros** — Claude uses its **browser** to open each shortlisted role's company on LinkedIn and
   surface **mutual connections** who could introduce the user.
6. **Track** — Claude writes a **spreadsheet** of the pipeline (role, match, contact, status).

Our scope to build is **steps 2–4** (the MCP server + minimal UI). Steps 1, 5, 6 are *Claude-side
orchestration* (conversation + browser + sheets skill) — we enable them, we don't implement them in the
backend. See §9 for that boundary and why it matters (LinkedIn ToS / auth).

---

## 2. Design principles

- **Token economics first.** Push high-volume, low-judgment work (filtering, embedding/pairwise ranking)
  to the *local* server and *cheap* models. Reserve the expensive cloud model for conversation and
  orchestration. Every tool result is shaped to minimize what Claude must read.
- **One core, many faces.** Search/rank logic lives in one Rust crate (`joblode-core`); the MCP tools, the
  REST/SSE API, and (later) a CLI are thin adapters over it. Define behavior once, expose it three ways.
- **MCP-native, with graceful fallback.** Tools return structured JSON *and* an MCP-UI resource. If a
  client doesn't render MCP-UI, the JSON still works.
- **TDD.** Each phase starts with the contract and failing tests, then the implementation. Data tests run
  on a tiny committed fixture parquet; model calls are mocked. (See §8.)
- **Rust + TypeScript monorepo, lightweight tooling.** Two languages only — a Rust backend and a React (TS)
  frontend — each using its native toolchain, orchestrated by Turbo under one dev environment (flox). No
  Python in the system. No Bazel unless we outgrow it.

### The Rust stack

| Concern | Choice |
|---|---|
| Columnar query + pushdown over the parquet | **DuckDB via the `duckdb` crate** (SQL + httpfs for remote R2); **DataFusion** is the pure-Rust alt — see §5 |
| Web server (REST + SSE + `/mcp`) | **axum** (tokio-native; first-class SSE; tower middleware) |
| MCP server SDK | **`rmcp`** (official Rust SDK; streamable-HTTP transport mounts into axum as a tower Service, plus stdio) |
| Shared logic | **`joblode-core` crate** (search / get / rank), reused by the web + MCP adapters |
| Async runtime | **tokio** (DataFusion/axum/rmcp are all tokio-native) |

---

## 3. System architecture

```
                       ┌─────────────────────────── Claude (cloud) ───────────────────────────┐
                       │  narrows criteria · calls MCP tools · browses LinkedIn · builds sheet │
                       └───────────────▲───────────────────────────────────▲──────────────────┘
                                       │ MCP (stdio or streamable HTTP)      │ browser + skills
                                       │                                     │ (NOT our backend)
         ┌─────────────────────────────┴─────────────────────────┐
         │              Rust service (single binary, axum)        │
         │  ┌──────────┐   ┌──────────┐   ┌──────────────────┐    │
         │  │ rmcp     │   │  REST +  │   │  MCP App (ui://)  │    │
         │  │ tools    │   │  SSE API │   │  resource builder │    │
         │  └────┬─────┘   └────┬─────┘   └────────┬─────────┘    │
         │       └───────────┬──┴──────────────────┘              │
         │            ┌──────▼───────┐   ┌──────────────┐         │
         │            │  joblode-core    │   │  joblode-rank   │         │
         │            │  search/get   │   │ Gemini (opt) │         │
         │            └──────┬───────┘   └──────┬───────┘         │
         │             ┌─────▼─────┐            │                 │
         │             │  DuckDB   │      cheap-model HTTP (reqwest)│
         │             └─────┬─────┘                              │
         └───────────────────┼────────────────────────────────────┘
                       open-jobs.parquet (~22GB, refreshed daily)

  React (Vite, minimal)  ── served by the Rust server; also the source for the MCP App ui:// resource
```

Boundaries:
- The Rust service owns **data + search + optional ranking + serving**. Nothing else.
- LinkedIn and the spreadsheet are **Claude's job** via its browser + a sheets/xlsx skill, acting as the
  logged-in user. We never store LinkedIn credentials or scrape server-side (§9).

---

## 4. The MCP server (Rust)

**SDK:** `rmcp`, the official Rust MCP SDK. Tools are defined with the `#[tool]` / `#[tool_router]` macros;
its streamable-HTTP transport is a tower `Service` that mounts straight into our axum app.

**Transports:** stdio (for local clients like Claude Desktop/Code) *and* streamable HTTP (mounted on the
same axum server as the web API). One core, both transports.

**Tools (the contract):**

| Tool | Input | Output | Notes |
|---|---|---|---|
| `search_jobs` | criteria (function, level, country, **city**, title, company, remote, min_comp, flags), `limit` | `{total, results[]}` compact rows + a UI resource | The hull filter. Sub-second. No key needed. |
| `get_job` | `id` | full record + `jd_markdown` | Lazy full detail for one role. |
| `rank_jobs` | `resume`, criteria *or* `ids`, `method` (`match`\|`pairwise`), `top` | ranked shortlist `{id, score, why}` | **Only if a cheap model is configured.** Server does the work; returns a short list. |
| `suggest_criteria` *(optional)* | `resume` | proposed criteria object | Helps Claude/the user narrow in step 1. Cheap-model call. |

`search_jobs` may also accept `rank: true` to fuse search+rank when a key is present — same effect as
calling `rank_jobs`, fewer round-trips.

**Token-aware result shaping:** results default to compact fields (id, company, title, location, comp,
one-line summary, score). Full JDs come only via `get_job`. This is the core token-saving move.

**MCP Apps (the official UI extension — confirmed supported by Claude).** As of the 2026-01-26 MCP Apps
spec, Claude (web/desktop/mobile) renders interactive UIs returned by MCP servers, and **React is a
first-class supported framework** (there's an official `basic-server-react` starter). So we target MCP
Apps with React directly — no need to settle for a static table. The developer contract:

- A tool declares `_meta.ui.resourceUri` pointing to a `ui://` resource (and `_meta.ui.csp` to allow our
  asset/font origins).
- The host fetches that `ui://` resource — an HTML page bundling our React app — and renders it in a
  **sandboxed iframe** inside the conversation.
- The iframe ↔ host talk over **postMessage**, a JSON-RPC dialect of MCP: the app can call our tools
  (`tools/call`), receive pushed tool results, and update the model's context. The
  `@modelcontextprotocol/ext-apps` `App` class wraps this (optional).
- **Always also return structured JSON** in the tool result. Host support still varies and there are
  early rendering bugs, so the data must stand alone if the iframe doesn't render. This is the spec's
  intended pattern (data + UI), not a fallback hack.

So our React build serves two jobs from one codebase: the `ui://` MCP App resource (talks to Claude via
the App bridge) and the standalone web UI (talks to our REST/SSE API). See §7.

**Config** (`config.toml` + env overrides):
```toml
parquet     = "open-jobs.parquet"
http_addr   = "127.0.0.1:8000"
mcp_http    = true            # also expose /mcp over HTTP
[rank]
provider    = "gemini"        # empty = ranking tools disabled
api_key_env = "GEMINI_API_KEY"
match_model = "gemini-2.5-flash"
pair_model  = "gemini-2.5-flash-lite"
```
"If the user sets up a cheap API" = this `[rank]` block being present. Absent ⇒ `rank_jobs` reports it's
unconfigured and `search_jobs` returns unranked.

---

## 5. Data layer — DuckDB (recommended)

**Decision: DuckDB via the `duckdb` crate** (with the `bundled` feature). It preserves every data/refresh
decision we made (DataFusion was the considered pure-Rust alternative — see end of section):
- Reads the parquet **directly** with projection + predicate **pushdown** and row-group pruning — the same
  "only touch the columns/rows that matter" trick we proved in Python, but free and in SQL.
- Our whole `search()` is one parameterized SQL query. Title and company terms are case-insensitive
  substring filters; multiple terms within one field are ORed, while different fields are ANDed.
  Company searches both `company_name` and raw `company`. The messy city filter is
  `lower(city || ' ' || region || ' ' || location) LIKE '%san francisco%'`.
- **`INSTALL httpfs`** lets it query the parquet straight off R2 (`FROM 'https://…/open-jobs.parquet'`) —
  this is what makes the "remote / zero-setup" refresh mode below trivial.
- Daily 22GB refresh is a non-event: query the file in place; no 1.4GB resident load, low memory.
- Trade-off: it **bundles libduckdb (C++)** at build time — longer first build, a C++ toolchain needed
  (flox pins it). Not pure Rust.

**Alternative: DataFusion** (pure Rust, arrow-native — the engine you liked the speed of). Also does
SQL + projection/predicate pushdown; remote parquet via the `object_store` crate (S3/R2/HTTP). Pros: no
C++ dependency, idiomatic Rust, fast incremental builds. Cons: the remote-R2-in-place path is a bit more
wiring than DuckDB's one-line `httpfs`. Pick this if "pure Rust, no bundled C++" matters more than the
zero-config httpfs convenience.

### Daily refresh & "cheapest read-only DB"

The cheapest read-only database for this *is* the Parquet file on object storage — there is no DB server
to run or pay for. The dataset already lives on R2 (`download.jobscream.com`, overwritten daily in place).
DuckDB is just the query layer on top, and it supports two modes from the same config:

- **Local file (default, recommended).** A daily job refreshes the local parquet; DuckDB queries it from
  disk (fast, no per-query egress). Refresh is one command: `task refresh` wraps the existing resumable
  `download.py` and does an atomic replace. We ship a cron/launchd snippet so a user sets up "fetch new
  data every morning" in one step. The server detects the file's mtime and re-opens its view, so a swap
  needs no restart.
- **Remote/direct (zero-setup).** Point `parquet` at the R2 URL; DuckDB's `httpfs` queries it **in place**
  over HTTP with projection + predicate pushdown and range requests, reading only the column chunks /
  row-groups it needs. "Daily refresh" is then automatic (upstream overwrites the file). R2 has no egress
  fees, so this is nearly free; the trade-off is slower than local and heavier when pulling JDs for
  ranking.

Config picks the mode by the value: `parquet = "open-jobs.parquet"` (local) or
`parquet = "https://download.jobscream.com/open-jobs.parquet"` (remote). DuckDB handles both identically,
so the rest of the code doesn't care. (Managed option if ever wanted: MotherDuck — hosted DuckDB — but
it's unnecessary for read-only Parquet.)

---

## 6. Ranking layer (Gemini, config-gated)

Port the two proven Python rankers to Rust (they're just HTTP + small math — `reqwest` + `serde`, no heavy deps):
- **`match`** — per-role 0–100 + verdict + strengths/gaps (absolute; fast; streams).
- **`pairwise`** — `langsort`-style "which fits better?" comparisons aggregated by Bradley-Terry
  (`btrank`). Better calibrated; the project's preferred ranking.

Both call the cheap model over Gemini's OpenAI-compatible endpoint. The **embedding-based recall**
(`rank.py`'s lexical-seed → ridge ranker → score the corpus) and the **distilled corpus-wide taste**
(`btrank --distill`: PCA + logistic regression over the job embeddings) also port to Rust — they're just
linear algebra over the embeddings already in the parquet, read for the candidate set via DuckDB/arrow.
Use **`nalgebra`** (pure Rust — SVD for the PCA, plus straightforward logistic/ridge + Bradley-Terry
iterations) so we pull in **no BLAS/LAPACK system dependency**.

**There is no Python in the running system.** The current scripts (`hull` / `rank` / `btrank` /
`langsort` / `match`) stay in git history purely as a one-time **porting reference and test oracle** —
Rust ranking is validated against the proven Python output on the fixture — and are deleted once Rust
reaches parity.

---

## 7. Frontend (React, minimal — one app, two runtimes)

- Vite + React + TypeScript, starting from the official `basic-server-react` MCP App template; reuse the
  current `index.html` UX (filter sidebar, streaming results table, detail drawer) as components.
- **One build, two runtimes**, detected at boot:
  - *Inside Claude* (MCP App): bundled into the `ui://` resource, runs in the sandboxed iframe, calls our
    tools and receives results via the `@modelcontextprotocol/ext-apps` App bridge (postMessage).
  - *Standalone web app* at `/`: same components, but data via the REST/SSE API.
  - A thin data-source adapter (`bridge` vs `http`) is the only branch; components don't know the
    difference.
- Keep it genuinely minimal — the heavy lifting and "intelligence" live in Claude + the server.

---

## 8. TDD plan — phases, each contract-and-tests-first

> Rhythm per phase: write the interface + failing tests → implement → green → refactor. Data tests use a
> **small committed fixture parquet** (hand-picked rows with known values). Model calls are
> **mocked** (deterministic fake comparator/scorer, as we did in the Python validation).

- **Phase 0 — Monorepo skeleton — complete.** flox toolchains (rust, node, duckdb), Cargo workspace,
  pnpm workspace, Turbo pipeline, CI (build+lint+test). *Tests:* `turbo test` green on empty stubs
  (`cargo test`, `vitest`); `turbo build` builds everything; `cargo clippy`/`fmt` clean.
- **Phase 1 — `joblode-core` crate over DuckDB — complete.** `JobStore::open(parquet)`,
  `search(&Criteria) -> Result<(Vec<Job>, usize)>`, and `get_job(id) -> Result<Job>`. The bundled
  DuckDB build includes the Parquet extension, so local searches require no runtime extension install.
  *Tests:* 11 integration cases over `testdata/fixture.parquet` cover city/function/level/title/company
  filters, title+company composition, US-remote-scope matching, comp-floor sentinel handling,
  case-insensitive `(company,title)` dedup, empty results, and full-JD retrieval.
- **Phase 2 — MCP server (`rmcp`).** `search_jobs` + `get_job` tools (JSON only), stdio + HTTP. *Tests:*
  invoke tools through an in-process transport; assert result schema and that `get_job` returns the JD.
- **Phase 3 — REST + SSE + React.** axum `/api/search`, `/api/job/:id`, serves the React build. *Tests:*
  axum handler tests (`tower::ServiceExt::oneshot`); one Playwright smoke (search → rows → open drawer).
- **Phase 4 — ranking (config-gated).** `joblode-rank` match + pairwise; `rank_jobs` tool; `rank` param on
  search; SSE streaming for the web UI. *Tests:* mock the model client (trait + fake impl); assert (a)
  pairwise recovers a planted order, (b) ranking disabled cleanly when no key, (c) token-shaped compact
  output.
- **Phase 4b — embedding recall + distilled ranker (optional, `nalgebra`).** Port `rank.py` (lexical-seed
  ridge ranker in embedding space) and `btrank --distill` (PCA + logistic over embeddings → score the whole
  corpus with no LLM calls). *Tests:* recovers a planted signal; parity with the Python oracle on the
  fixture (within tolerance). Not needed for the core search→match→pairwise flow; build when wanted.
- **Phase 5 — MCP App UI.** Declare `_meta.ui.resourceUri`; serve the React bundle as a `ui://` resource;
  the iframe calls tools via the App bridge. *Tests:* resource payload shape/mime; the React data-source
  adapter (bridge vs http); JSON in the tool result regardless.
- **Phase 6 — Orchestration enablement (docs, not backend).** An MCP usage guide + example Claude prompts
  for the full flow (criteria → search → rank → LinkedIn intros → spreadsheet). The LinkedIn/sheet steps
  are Claude's browser + skills; we ship guidance and verify the tools they call behave.

---

## 9. Boundaries, safety, privacy

- **LinkedIn is Claude-side, as the user.** Mutual-connection lookup happens in Claude's browser session
  logged in as the user — *not* server-side scraping. Rationale: LinkedIn ToS forbids automated server
  scraping; the user's own authenticated, human-paced browsing via the agent is the appropriate path. The
  Rust backend stores no LinkedIn credentials and makes no LinkedIn calls.
- **Secrets.** Model API keys come from env (referenced by config), never written to disk or logs. The
  resume is sent only to the local server and the user's own model key.
- **Bind local.** Default `127.0.0.1`. The MCP-over-HTTP endpoint is a tool surface — no `0.0.0.0` without
  auth.
- **Data honesty (from AGENTS.md).** Structured fields are LLM extractions; comp/auth/location must be
  confirmable against `jd_markdown`. The `url` is the only apply link; never fabricate roles.

---

## 10. Proposed repo layout

```
joblode/
  .flox/                      # dev env: rust, node, pnpm, duckdb, cargo-deny, cargo-llvm-cov
  .github/                    # CI, CodeQL, Scorecard, dependabot, CODEOWNERS, issue/PR templates
  CLAUDE.md                   # lean agent guide (points here for architecture)
  Cargo.toml                  # Rust workspace (members = crates/*)
  deny.toml                   # cargo-deny: advisories / licenses / bans / sources
  package.json                # root: turbo + scripts
  pnpm-workspace.yaml         # JS workspace
  turbo.json                  # JS task graph + cache
  docs/DESIGN.md              # this file
  crates/
    joblode-core/            # search / get / rank logic over DuckDB  (lib; tests inline + tests/)
    joblode-server/          # axum: REST + SSE + rmcp (stdio & HTTP) + ui:// resource  (bin)
    joblode-mcp/             # optional stdio-only MCP bin (same joblode-core)  [added when needed]
  testdata/fixture.parquet   # tiny committed Phase 1 data fixture
  web/                        # React (Vite, TS) — one build: web UI + MCP App ui:// resource
```
Turbo orchestrates the JS workspace (`web/`); Cargo builds the Rust workspace; CI runs both. **No Python
in this repo** — the original Python scripts live in the separate `open-jobs` repository and serve as the
**porting oracle** (validate Rust ranking against the proven Python output on the fixture) until the Rust
port reaches parity.

---

## 11. Decisions — resolved 2026-06-21

**Languages: Rust (entire backend) + TypeScript/React (frontend). No Python in the system** — the existing
scripts survive only as a git-history porting reference / test oracle and are removed at parity.

1. **Data engine: DuckDB via the `duckdb` crate** (locked) — keeps the daily-refresh story trivial: query
   Parquet in place, local *or* remote R2 via `httpfs`, no ETL, no DB server. "Cheapest read-only DB" =
   object storage + DuckDB. Both refresh modes shipped via config (§5). DataFusion was the considered
   pure-Rust alternative; not chosen.
2. **All logic in Rust — Python removed.** `match` + `pairwise` via `reqwest`; embedding recall + the
   distilled corpus-wide ranker via `nalgebra` (pure Rust, no BLAS/LAPACK). One self-contained binary. The
   old Python scripts are a temporary porting reference/oracle only (§6), deleted at parity.
3. **UI: MCP Apps with React, from the start** (verified Claude-supported, React is first-class), with
   structured JSON always in the tool result. One React build serves both the MCP App `ui://` resource and
   the standalone web UI (§7).
4. **Monorepo tooling: Turborepo** for task orchestration + (remote) caching, over a **Cargo workspace**
   (Rust) and a pnpm workspace (JS), under flox. Note: Turbo is JS-centric — Rust tasks are wrapped as
   `package.json` scripts with explicit input/output globs so Turbo can cache them; its affected-detection
   for Rust is coarser than a polyglot-native tool (moon/Bazel). Acceptable for a two-language repo;
   revisit moon if it grows.
5. **MCP SDK: `rmcp`** (official Rust SDK). Streamable-HTTP transport mounts into axum; stdio for local
   clients. Open item: confirm it lets us set tool `_meta.ui.*` for MCP Apps; if not yet, set the raw
   `_meta` directly (it's just tool metadata) and serve the `ui://` resource via standard MCP resources.

Open follow-ups (don't block Phase 0): exact `_meta.ui` wiring in `rmcp`. (Shortlist/status ownership is
settled in §13 — Claude owns it; no `export`/`track` tool for now.)

---

## 12. Resource footprint (local)

The file is 22 GB but **we never load it into RAM** — DuckDB queries the parquet with projection +
predicate pushdown and row-group pruning, so search reads only the *structured* column chunks of
*matching* row groups. The embedding columns (the ~20 GB bulk) are never read for search. (Contrast: the
old Python server held 1.43 GB resident permanently.)

| Resource | Local mode | Remote mode (`httpfs` off R2) |
|---|---|---|
| Disk — data | ~22 GB (the parquet) | ~0 (streamed; small temp) |
| Disk — app | Rust binary + bundled libduckdb ~60–120 MB; React build a few MB | same |
| Disk — dev toolchains | flox/nix store several GB (dev only; not needed to *run* a built binary) | same |
| RAM — idle | tens of MB (Rust process) | same |
| RAM — per query | ~200 MB–1 GB peak, **bounded by DuckDB `memory_limit`** (set ~1–2 GB) | + network buffers |
| RAM — ranking | a few MB (match/pairwise); ≤~150 MB transient for embedding recall (Phase 4b) | same |
| OS page cache | uses spare RAM to cache hot parquet pages (reclaimable; speeds repeats) | n/a |

**Bottom line:** comfortable on **8 GB** RAM, ideal on 16 GB. Disk **~23 GB local**, or **<1 GB in remote
mode**. Expose DuckDB `memory_limit` and `threads` in config so it stays bounded on small machines.

---

## 13. State ownership — us vs Claude (Claude-only deployment)

**Principle: the server is stateless. We own _reads + rendering_; Claude owns _writes + durable user
state_.** Split by the kind of data:

| Concern | Owner | How |
|---|---|---|
| Job corpus (read-only, ~1M, daily refresh) | **Us** | DuckDB over parquet — Claude can't hold this |
| `search` / `get` / `rank` | **Us** | MCP tools |
| Result + shortlist **views** (interactive table/board) | **Us** | MCP App, *rendered from input*; captures the user's selections/status changes and surfaces them to Claude |
| Criteria narrowing | **Claude** | conversation |
| Which jobs are shortlisted | **Claude** | spreadsheet + project memory |
| Application **status / notes / contacts** | **Claude** | a spreadsheet (editable, portable, persists in Drive) + memory |
| LinkedIn mutual connections | **Claude** | browser, as the logged-in user (§9) |
| Status changes over time | **Claude** | edits the sheet / memory |

**Why push state to Claude rather than build it ourselves:**
- **Less code for us** — no write path, storage, auth, multi-user, sync, or migrations. The server stays a
  pure read-only query+rank service: easy to build TDD, test, and operate.
- **Better UX** — the tracking artifact is a *real spreadsheet the user owns*: sort/edit by hand, share,
  keep after the chat. Data trapped in our server would be worse for exactly this need. It's also the
  stated goal ("create spreadsheet to track progress").
- **Claude is good at this** — maintaining a structured doc and cross-referencing tools is a natural agent
  task.

**Stateless but still feels stateful:** in the MCP App, checking roles or setting a status doesn't write
to *our* DB — the iframe pushes a context update / message to Claude (the MCP Apps spec supports app→host
messages), and Claude persists to the sheet. Optionally a `render_shortlist(items)` tool renders a rich
interactive board from a list **Claude supplies** (read from the sheet): *we provide the view, Claude
provides the data*; edits flow back to Claude → sheet.

**When we'd add state on our side** (out of scope for Claude-only): a standalone web app, or server-side
automation like daily digest emails (JobScream-style). Then add a small SQLite-backed `shortlist`/`status`
tool. Not now.

---

## 14. Engineering workflow & repo quality

This is a public repo, so the engineering process is part of the artifact. The bar: **`main` stays green,
checks gate every change, and the setup reads as professional at a glance.**

**CI (every push + PR), all free for public repos:**
- **Rust** — `cargo fmt --check`, `cargo clippy -D warnings`, tests via `cargo llvm-cov` (coverage), build.
- **Web** — ESLint, strict `tsc`, Vitest (coverage), Vite build, orchestrated by Turbo.
- **cargo-deny** — advisories / licenses / bans / sources over the dependency tree (`deny.toml`).
- **CodeQL** — security analysis (TypeScript today; Rust when GA).
- **Codecov** — coverage diff on PRs + a badge (makes the TDD discipline visible).

**Scheduled / on main:**
- **OpenSSF Scorecard** — security-posture score + badge.
- **Dependabot** — weekly grouped updates for cargo, npm, and GitHub Actions.

**AI review:** **CodeRabbit** (free full-Pro on public repos) reviews every PR; `.coderabbit.yaml` configures it.

**Branch protection on `main`:** require the CI checks (`rust`, `web`, `cargo-deny`) to pass and a PR
before merge (0 required human approvals — solo-friendly). Direct pushes to `main` are disallowed; work
flows through PRs, one per phase/task.

**Agent setup:** a **lean** [`CLAUDE.md`](../CLAUDE.md) is the per-session constitution (behavioral
guardrails + repo map + commands) and points here for architecture — it deliberately does *not* duplicate
this doc. Heavy personal agent tooling (e.g. Garry Tan's gstack) is kept at the developer's *global* level,
not committed to this repo, to keep the public tree clean; a small curated `.claude/` may be added later if
a project-specific skill earns its place.
