# Run joblode & use it with Claude

How to run the joblode MCP server locally and connect Claude to it so you can search the open-jobs
dataset (~1M live roles) from a conversation. For architecture and the roadmap, see
[DESIGN.md](DESIGN.md).

The server exposes three MCP tools today:

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

The in-conversation React UI lands in a later phase (DESIGN §8).

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

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `JOBLODE_PARQUET` | `open-jobs.parquet` (relative to the working dir) | Path to the dataset. Use an absolute path when launched by Claude. |
| `JOBLODE_HTTP_ADDR` | `127.0.0.1:8000` | Bind address for the `http` transport (loopback only). |
| *(argument)* | `stdio` | Transport: `stdio` or `http`. |
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
