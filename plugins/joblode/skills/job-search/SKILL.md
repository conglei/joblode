---
name: job-search
description: Run an agent-driven job hunt over the joblode dataset (~1M live roles). Use when the user wants to find jobs, search roles by description, shortlist/rank matches, validate roles with thumbs up/down, or track applications. Drives the joblode MCP tools (search_jobs, semantic_search, rank_jobs, get_job).
---

# joblode — agent job search

You drive the hunt; the user supplies intent and reactions. The joblode MCP server
owns search + ranking + the interactive results card. Keep the candidate set on the
server and reduce it before reading details — never read dozens of full JDs to sort
them.

## Tools

- **`search_jobs`** — hard filters (function, level, title, company, city, country,
  `min_comp`) → a total count + compact rows. The hull filter; no key needed.
- **`semantic_search`** — match a free-text *description of the work* against role
  embeddings by meaning, under the same filters. Use when the structured fields
  don't filter cleanly. Needs an embeddings key.
- **`rank_jobs`** — reduce a candidate set to a short, ordered shortlist `{id, score,
  why}`. Pass the user's reactions as `feedback: [{id, label}]` (`liked`/`disliked`)
  and it personalizes **for free** via a taste ranker — no key. Optional cheap-model
  `match`/`pairwise` refine the top (need a key + a resume).
- **`get_job`** — one role's full record incl. `jd_markdown`.

The result-returning tools render an **interactive card** in the conversation when
the host supports it (claude.ai / Desktop): a results table with 👍/👎 per role.

## The loop — surface a little, learn, then more

**Don't overwhelm.** The dataset is huge, so it's tempting to run several searches and
dump 40–50 rows each. Don't. The user can't triage hundreds of JDs. Surface a **small
batch (~8–10)**, learn from their reactions, then show the next batch.

1. **Narrow.** Talk to the user; converge on hard filters (and a one-line description
   of the work if it's fuzzy). Don't guess silently — confirm the filters.
2. **Search broad, surface narrow.** `search_jobs` for clean filters, or
   `semantic_search` when meaning matters — keep the *candidate set* broad (that's
   cheap to rank). But then **`rank_jobs` with `top: 8–10`** and show only that small
   shortlist. If you ran several criteria, consolidate into **one** ranked batch, not
   one card per criterion.
3. **Validate with the user.** Present that small batch and invite 👍/👎. In the card
   the user reacts directly; otherwise ask which look good. A handful of reactions is
   plenty to start.
4. **Re-rank, then show the next batch.** Feed the accumulated
   `feedback: [{id, label}]` back into `rank_jobs` — the taste ranker reorders the
   *whole* candidate set from a few reactions. Surface the next ~8–10 the user hasn't
   seen. Repeat: each round is sharper because it learns from the last.
5. **Read the few that matter.** `get_job` for the ones the user liked. **Confirm comp,
   work authorization, and location against `jd_markdown`** — structured fields are LLM
   extractions and can be wrong. The `url` is the only apply link; never invent roles.
6. **Track.** Maintain a spreadsheet (role, company, match, apply link, status) and the
   user's running taste, so later searches start from what they liked.

The point: a tight **show a few → react → learn → show a few more** loop, not a wall of
results. Stop when the user has enough strong matches.

## Remember the user's taste

The user's 👍/👎 is durable preference, and it's **yours to carry** — joblode is
stateless. Keep the liked/disliked role ids (in the tracking sheet / your memory) and
pass them into every `rank_jobs` call, so each search is personalized by everything
they've reacted to so far. When the interactive card is shown, it surfaces the user's
reactions back to you as context — fold them into the running feedback.

## Warm intros & tracking (your job, not the server)

For the shortlist, use your browser **as the user** to find LinkedIn mutual
connections (never server-side scraping), and keep the pipeline in a spreadsheet the
user owns. See the joblode orchestration guide for the full flow.
