# joblode plugin

Packages joblode for one-step install in Claude Code: the **joblode MCP server**
(`search`, `rank_jobs`, `get_job` + the interactive results
card) plus a **job-search skill** and a `/job-search` command.

## Install

```
/plugin marketplace add conglei/joblode
/plugin install joblode@joblode
```

## Prerequisites (run the server)

The plugin connects to a running joblode server over HTTP (`http://127.0.0.1:8000/mcp`),
so start it first — one server backs the web UI, the claude.ai connector, and this
plugin:

1. Build it: `cargo build --release -p joblode-server`.
2. Configure via a gitignored `.env` (or exports): `JOBLODE_PARQUET` (required),
   `JOBLODE_EMBED_PROVIDER=openai` + `OPENAI_API_KEY` (semantic search),
   `JOBLODE_RANK_PROVIDER=gemini` + `GEMINI_API_KEY` (optional refine).
3. For fast semantic search, build the sidecar once: `joblode-server build-sidecar`.
4. Run it: `joblode-server http` (binds `127.0.0.1:8000`).

See [`docs/MCP.md`](../../docs/MCP.md) for full configuration and
[`docs/ORCHESTRATION.md`](../../docs/ORCHESTRATION.md) for the workflow.

> Prefer Claude Code to manage the process itself? Swap `.mcp.json` for a stdio
> launch — `{ "command": "joblode-server", "args": ["stdio"], "env": { … } }` — with
> the binary on your `PATH`. HTTP is the default because one server serves every face.

## Surfaces

- **Claude Code / Cowork** — the plugin bundles everything (MCP server auto-connects,
  skill + command available).
- **claude.ai / Claude Desktop chat** — the plugin system doesn't auto-run a bundled
  MCP server there; add joblode as a **custom connector** manually (see
  [`docs/MCP.md`](../../docs/MCP.md)). The skill's workflow still applies.
