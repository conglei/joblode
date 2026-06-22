# Run joblode & use it with Claude

How to run the joblode MCP server locally and connect Claude to it so you can search the open-jobs
dataset (~1M live roles) from a conversation. For architecture and the roadmap, see
[DESIGN.md](DESIGN.md).

For the full agent-driven workflow (narrow → search → rank → intros → track) and copy-paste prompts, see
[ORCHESTRATION.md](ORCHESTRATION.md).

The server exposes four MCP tools:

- **`search_jobs`** — hard filters (function, level, title, company, city, country, min comp) → a total
  match count plus compact rows (`limit`-capped, default 50).
- **`get_job`** — one role by `id`, including its full `jd_markdown`.
- **`rank_jobs`** — reduces a candidate set to a compact, ordered shortlist `{id, score, why}` so the
  cloud model reads dozens of rows, not thousands. Draws candidates by the same filters (or explicit
  `ids`), orders them **for free** against prior `feedback` (`[{id, label}]`, where label is
  `liked`/`applied`/… or `disliked`/`rejected`/…), and — if a cheap model is configured — refines the
  top with `method: "match"` or `"pairwise"` (these also need a `resume`). Without a key, the free
  feedback-driven ranking still works.
- **`semantic_search`** — matches a free-text `query` (a description of the role/responsibilities) against
  role embeddings by cosine similarity, scoring each role by its **best-matching variant** (title, JD, or
  an alternate title) — useful when the messy structured fields don't filter cleanly. Takes the same hard
  filters; returns compact rows with a `score`. **Requires an embeddings key** (see config).

When a host supports MCP Apps, the result-returning tools (`search_jobs`, `semantic_search`, `rank_jobs`)
also render an **interactive results table** in the conversation — see [Run the web UI](#run-the-web-ui-optional).

## 1. Get the dataset

The server reads the open-jobs dataset straight from a local Parquet file — there is no database to run.

- Obtain the open-jobs parquet (~22 GB; see [DESIGN §5](DESIGN.md#5-data-layer--duckdb-recommended) for the
  source) and place it at the **repo root** as `open-jobs.parquet`. That is the default path, so a server
  started from the repo root finds it with no configuration.
- The file is git-ignored (`*.parquet`) — never commit it.
- To keep it elsewhere, set `JOBLODE_PARQUET` to an absolute path (see [Configuration](#configuration)).

## 2. Build the server

```bash
flox activate                      # provides cargo, node, pnpm, duckdb
cargo build -p joblode-server --release
# binary: target/release/joblode-server
```

## 3. Run it

The binary takes one argument — the transport:

```bash
# stdio — for local MCP clients like Claude Desktop / Claude Code
./target/release/joblode-server stdio

# streamable HTTP — mounted at /mcp (default 127.0.0.1:8000)
./target/release/joblode-server http
```

Run from the repo root to use the default `open-jobs.parquet`, or pass the path explicitly:

```bash
JOBLODE_PARQUET=/abs/path/to/open-jobs.parquet ./target/release/joblode-server stdio
```

> The server binds to `127.0.0.1` only. The HTTP endpoint is an unauthenticated tool surface — don't expose
> it beyond localhost.

## 4. Enable it in Claude

When Claude launches the server, its working directory is **not** the repo root, so always give the dataset
as an **absolute** path via `JOBLODE_PARQUET`.

### Claude Code (CLI)

```bash
claude mcp add joblode \
  --env JOBLODE_PARQUET=/abs/path/to/joblode/open-jobs.parquet \
  -- /abs/path/to/joblode/target/release/joblode-server stdio
```

Verify with `claude mcp list`, then start a session and ask Claude to search.

### Claude Desktop

Edit `claude_desktop_config.json` (macOS:
`~/Library/Application Support/Claude/claude_desktop_config.json`) and add:

```json
{
  "mcpServers": {
    "joblode": {
      "command": "/abs/path/to/joblode/target/release/joblode-server",
      "args": ["stdio"],
      "env": { "JOBLODE_PARQUET": "/abs/path/to/joblode/open-jobs.parquet" }
    }
  }
}
```

Restart Claude Desktop; "joblode" appears in the tools menu.

### Any HTTP MCP client (e.g. MCP Inspector)

```bash
./target/release/joblode-server http        # from the repo root
npx @modelcontextprotocol/inspector          # point it at http://127.0.0.1:8000/mcp
```

## 5. Try it

Once connected, drive it from the conversation — for example:

- "Search joblode for senior backend engineer roles in the US, show me 10."
- "Filter to San Francisco, product function, comp floor 180k."
- "Open the full description for that third result."

Claude calls `search_jobs` to draw the candidate set, then `get_job` for the roles you want to read in
full. Structured fields are LLM extractions — confirm comp, work authorization, and location against
`jd_markdown`, and use the `url` (the only apply link) to apply.

## Run the web UI (optional)

One React build serves two runtimes (DESIGN §7): a **standalone web app** over the REST API, and the
**MCP App** `ui://` resource rendered inside Claude. Build both with one command:

```bash
pnpm --filter @joblode/web build
# → web/dist/      the standalone web app (multi-file)
# → web/dist-app/  the MCP App bundle (one self-contained index.html)
```

Then run the server over HTTP and open it:

```bash
./target/release/joblode-server http      # from the repo root
# standalone UI:  http://127.0.0.1:8000/
```

The `http` server serves `web/dist` at `/` (override with `JOBLODE_WEB_DIR`) and serves the MCP App bundle
as the `ui://joblode/app` resource, read from `web/dist-app/index.html` (override with `JOBLODE_APP_HTML`).
A host that supports MCP Apps fetches that resource and renders the table in the conversation; one that
doesn't still gets the structured JSON in every tool result, so nothing breaks. Building the UI is optional
— the tools work headless without it.

## Speed up semantic search (build the sidecar)

Semantic search over the full ~1M-role corpus is a brute-force cosine scan of the embedding columns — slow
and CPU-bound when run without hard filters. Build a **compact embedding sidecar** once and it gets ~10–50×
faster: a single pass writes each role's `jd_embedding` truncated to 256 dims (the Matryoshka prefix of the
1536-d vectors — still a usable embedding) as a small `FLOAT[256]` file next to the dataset.

```bash
# Build it once (and after each daily data refresh):
./target/release/joblode-server build-sidecar     # writes open-jobs.parquet.emb.parquet
```

The server **auto-attaches** the sidecar on startup if it exists (look for `attached embedding sidecar` in
the log); otherwise it warns and falls back to the slow full scan. Semantic search then ranks off the small
file (and still applies your hard filters first). Tune the dimension with `JOBLODE_EMBED_DIM` (smaller =
faster, slightly less precise). The sidecar stays parquet-in-place — no database, no daily index rebuild
beyond re-running `build-sidecar`.

> Quality note: the sidecar ranks on `jd_embedding` only (the richest signal), whereas the unindexed path
> scores the best of title / JD / alternate-title embeddings. Build the sidecar for speed; delete it (or
> unset `JOBLODE_EMBED_SIDECAR`) to fall back to the multi-variant scan.

## Configuration

Config is read from the environment. At startup the server also loads a gitignored
`.env` from the working directory if present — copy [`.env.example`](../.env.example)
to `.env` and fill in keys (real environment variables take precedence).

| Variable | Default | Meaning |
|---|---|---|
| `JOBLODE_PARQUET` | `open-jobs.parquet` (relative to the working dir) | Path to the dataset. Use an absolute path when launched by Claude. |
| `JOBLODE_HTTP_ADDR` | `127.0.0.1:8000` | Bind address for the `http` transport (loopback only). |
| `JOBLODE_WEB_DIR` | `web/dist` | Standalone web app served at `/` over HTTP. |
| `JOBLODE_APP_HTML` | `web/dist-app/index.html` | MCP App bundle served as the `ui://joblode/app` resource. |
| *(argument)* | `stdio` | Command: `stdio`, `http`, or `build-sidecar` (build the semantic-search index, then exit). |
| `JOBLODE_EMBED_SIDECAR` | `<parquet>.emb.parquet` | Compact embedding sidecar for fast semantic search (see below). Auto-attached on startup if present. |
| `JOBLODE_EMBED_DIM` | `256` | Truncated embedding dimension `build-sidecar` writes (Matryoshka prefix of the 1536-d vectors). |
| `JOBLODE_RANK_PROVIDER` | *(unset)* | Set to `gemini` to enable the `match`/`pairwise` ranking methods. |
| `GEMINI_API_KEY` | *(unset)* | Cheap-model key (override the var name with `JOBLODE_RANK_API_KEY_ENV`). |
| `JOBLODE_RANK_MATCH_MODEL` | `gemini-2.5-flash` | Model for the absolute `match` pass. |
| `JOBLODE_RANK_PAIR_MODEL` | `gemini-2.5-flash-lite` | Model for the `pairwise` pass. |
| `JOBLODE_RANK_BASE_URL` | Gemini OpenAI-compatible endpoint | Override for an OpenAI-compatible base URL. |
| `JOBLODE_EMBED_PROVIDER` | *(unset)* | Set to `openai` to enable `semantic_search` / `/api/semantic`. |
| `OPENAI_API_KEY` | *(unset)* | Embeddings key (override the var name with `JOBLODE_EMBED_API_KEY_ENV`). |
| `JOBLODE_EMBED_MODEL` | `text-embedding-3-small` | Query embedding model — must match the dataset's vectors (1536-d). |
| `JOBLODE_EMBED_BASE_URL` | OpenAI `/v1` | Override for an OpenAI-compatible embeddings base URL. |

## Notes & limits

- **Local file only for now.** Querying the dataset directly off remote object storage (DuckDB `httpfs`,
  DESIGN §5) isn't wired yet — point `JOBLODE_PARQUET` at a local file.
- **Ranking is config-gated.** The free, feedback-driven `rank_jobs` works with no key; the `match` and
  `pairwise` methods need `JOBLODE_RANK_PROVIDER`/`GEMINI_API_KEY` and a `resume`.
- **Server start re-validates the file.** A missing or unreadable parquet fails fast with a clear error.
