---
name: job-search
description: Run an agent-driven job hunt over the joblode dataset (~1M live roles). Use when the user wants to find jobs, search roles by description, shortlist/rank matches, validate roles with thumbs up/down, or track applications. Drives the joblode MCP tools (search, rank_jobs, get_job).
---

# joblode — agent job search

You drive the hunt; the user supplies intent and reactions. The joblode MCP server
owns search + ranking + the interactive results card. Keep the candidate set on the
server and reduce it before reading details — never read dozens of full JDs to sort
them.

## Tools

- **`search`** — one search, two match modes: hard filters (function, level, title,
  company, city, country, `min_comp`) for keyword/structured match, **plus an optional
  `query`** for semantic match against the job description. Filter-only needs no key;
  a `query` orders by similarity and attaches a `score` (needs an embeddings key).
- **`rank_jobs`** — the **finalization** step. Once the criteria are settled, rank the
  **whole** matching set into an ordered shortlist `{id, score, why}` by the user's
  taste — learned **for free** from `feedback: [{id, label}]` (`liked`/`disliked`).
  Fast, keyless, **no resume**. Pass `top` (default 25; ask for ~100 for a final list).
  (`match`/`pairwise` are an optional cheap-model refine — need a key + resume, much
  slower; rarely worth it.)
- **`get_job`** — one role's full record incl. `jd_markdown`.

The result-returning tools render an **interactive card** in the conversation when
the host supports it (claude.ai / Desktop): a results table with 👍/👎 per role.

## Two stages — explore, then finalize

Keep **search** and **rank** distinct:

- **Explore (search):** figure out *what* to look for. Surface a small batch, let the
  user + you react, refine the filters, repeat.
- **Finalize (rank):** once the criteria are right, order *everything* that matches.

### Explore — surface a little, learn, refine

**Don't overwhelm.** The dataset is huge; don't run several searches and dump 40–50 rows
each. Surface a **small batch (~8–10)**, learn, adjust.

1. **Narrow.** Talk to the user; converge on hard filters (and a one-line description of
   the work if it's fuzzy). Don't guess silently — confirm the filters.
2. **Search.** `search` — filters for clean criteria, plus a `query` when meaning
   matters. Show a small batch — not a card per criterion.
3. **Validate.** Present that batch and invite 👍/👎 (the user reacts in the card, or
   tell me which look good). A handful of reactions is plenty.
4. **Refine the criteria.** Reason over the reactions: what do the liked ones share?
   Adjust the filters / description and search again. Repeat until the criteria feel
   right — a few rounds, not dozens of results at once.

### Finalize — rank the whole set

5. **Rank.** Call `rank_jobs` with the **settled filters** and all accumulated
   `feedback: [{id, label}]`. It orders the **whole** matching set by taste and returns
   the final shortlist (ask for `top: ~100` if you want a broad list). One call — this
   is the output, not another exploration round.
6. **Read the few that matter.** `get_job` for the top picks. **Confirm comp, work
   authorization, and location against `jd_markdown`** — structured fields are LLM
   extractions and can be wrong. The `url` is the only apply link; never invent roles.
7. **Track.** Maintain a spreadsheet (role, company, match, apply link, status) and the
   user's running taste, so later hunts start from what they liked.

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
