# Running the hunt with Claude

joblode is built to be **driven by Claude**, not used as a standalone search box. The Rust server owns
steps 2–4 (search · rank · render); Claude owns the conversation around them — narrowing criteria, learning
your taste, finding warm intros, and tracking the pipeline. This guide is the playbook: the end-to-end flow
and example prompts you can paste into a session.

For how to install and connect the server, see [MCP.md](MCP.md). For the architecture and the boundary
between "us" and "Claude," see [DESIGN.md](DESIGN.md) §1 and §13.

---

## The flow at a glance

```text
1. Narrow      ── conversation ──────────────────────────────►  hard filters
2. Search      ── search_jobs / semantic_search ─────────────►  candidate set (compact rows)
3. Rank        ── rank_jobs (feedback + optional cheap model) ►  short, ordered shortlist
4. Read        ── get_job ────────────────────────────────────►  full JD for the few that matter
5. Intros      ── Claude's browser, as you, on LinkedIn ──────►  mutual connections
6. Track       ── a spreadsheet Claude maintains ────────────►  role · match · contact · status
```

Steps 2–4 are our MCP tools. Steps 1, 5, 6 are **Claude-side** — conversation, its browser, and a sheet.
The server is stateless: your shortlist and application status live in the sheet and Claude's memory, never
on our side (DESIGN §13).

---

## 1. Narrow the criteria

Don't start by searching — start by telling Claude what you want and handing it your resume. Let it propose
the hard filters before drawing a candidate set.

> "Here's my resume \[paste / attach]. I'm looking for senior backend or platform roles, US-remote or NYC,
> comp floor around $180k. Help me turn that into a search."

Claude converges on a `search_jobs` filter set (function, level, city, country, `min_comp`, title/company
terms). Push back if it guesses wrong — criteria narrowing is a conversation, and the hard filters only
change here, not in the ranker.

## 2. Search — hard filters, then meaning

Two complementary tools draw the candidate set:

- **`search_jobs`** for clean structured filters: "senior backend, US, comp ≥ 180k." Returns a **total match
  count** plus compact rows (capped, default 50) — enough to triage without reading full JDs.
- **`semantic_search`** when the structured fields don't filter cleanly: describe the *work* and match it
  against role embeddings by meaning. Needs an embeddings key (see [MCP.md](MCP.md#configuration)).

> "Search joblode: senior backend engineer, United States, comp floor 180k. How many match?"
>
> "Now semantic-search within those for *'owning data pipelines and event infrastructure, not CRUD APIs'*."

Keep the candidate set broad here (hundreds is fine) — the next step is what makes it cheap to sort.

## 3. Rank — spend tokens where they matter

Handing 300 full JDs to the cloud model is the expensive mistake joblode exists to avoid. `rank_jobs`
reduces a candidate set to a short, ordered shortlist `{id, score, why}` so Claude reads dozens of rows,
not thousands.

- **Free, keyless taste ranking.** Pass your reactions as `feedback: [{id, label}]` — `liked`/`applied`/
  `saved` (positive) or `disliked`/`rejected`/`skipped` (negative). The server learns a direction in
  embedding space (Rocchio) and reorders every candidate by it. No model calls, no key.
- **Optional cheap-model refinement** of the top: `method: "match"` (per-role 0–100 + a one-line reason) or
  `method: "pairwise"` (better-calibrated head-to-head). Both need a configured key and your `resume`.

> "Rank those candidates. I like jl_8831 and jl_2096, not interested in jl_5512 (too much management)."
>
> "Refine the top 15 with the pairwise method against my resume."

The feedback **is** the training signal, and it's yours — Claude carries it forward (in the sheet / its
memory) and passes it into each `rank_jobs` call, so the ranking sharpens as you react. Re-rank freely as
your taste clarifies.

## 4. Read the few that matter

Only now pull full descriptions, for the handful worth a close look:

> "Open the full JD for the top 3."

Claude calls `get_job` per role. **Confirm the important facts against `jd_markdown`** — comp, work
authorization, and location are LLM extractions and can be wrong. The `url` is the only real apply link;
Claude never invents roles.

### The in-conversation table (MCP Apps)

When the host supports MCP Apps (Claude web/desktop), the result-returning tools render an **interactive
table** in the conversation instead of a text dump — sort, scan, open a detail drawer, react to roles. It's
the same React app as the standalone web UI, served as a `ui://` resource and rendered in a sandboxed
iframe; your reactions flow back to Claude as feedback. Hosts that don't render it still get the structured
JSON, so the flow degrades cleanly. Build the bundle with `pnpm --filter @joblode/web build` (see
[MCP.md](MCP.md#run-the-web-ui-optional)).

## 5. Warm intros — Claude's browser, as you

For the shortlist, ask Claude to find people who could introduce you. This happens in **Claude's browser,
logged in as you** — not server-side scraping (LinkedIn ToS; DESIGN §9). The Rust backend stores no
LinkedIn credentials and makes no LinkedIn calls.

> "For each shortlisted company, open it on LinkedIn and list mutual connections who could intro me."

## 6. Track the pipeline — a spreadsheet you own

Ask Claude to keep a sheet as you go: role, company, match score, the JD `url`, your contact / intro path,
and status (researching → applied → interviewing → …). It's a real spreadsheet you keep after the chat —
sortable, shareable, portable — which is exactly why we *don't* build a tracking store on our side
(DESIGN §13).

> "Add the top 5 to a tracking sheet with company, title, match, apply link, and a status column."
>
> "Mark jl_2096 as applied; I heard back from jl_8831 — set it to interviewing."

---

## A full session, condensed

> 1. "Here's my resume. Senior backend/platform, US-remote or NYC, ~$180k floor — help me search joblode."
> 2. "Run the search. How many match?" → *Claude calls `search_jobs`.*
> 3. "Semantic-search within those for data-platform work, not CRUD." → *`semantic_search`.*
> 4. "Rank them; I like jl_8831 and jl_2096, not jl_5512." → *`rank_jobs` with feedback.*
> 5. "Open the top 3." → *`get_job`; Claude checks comp/auth against the JD.*
> 6. "Find LinkedIn mutuals at those companies." → *Claude's browser.*
> 7. "Build a tracking sheet with the top 5." → *Claude maintains the spreadsheet.*

Each turn, Claude reads the smallest result that answers it — the whole design is shaped so the expensive
model never has to read what the local server and cheap models can reduce first.
