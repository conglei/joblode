# joblode

[![CI](https://github.com/conglei/joblode/actions/workflows/ci.yml/badge.svg)](https://github.com/conglei/joblode/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/conglei/joblode/branch/main/graph/badge.svg)](https://codecov.io/gh/conglei/joblode)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/conglei/joblode/badge)](https://scorecard.dev/viewer/?uri=github.com/conglei/joblode)
[![CodeRabbit](https://img.shields.io/coderabbit/prs/github/conglei/joblode?labelColor=171717&color=FF570A&label=CodeRabbit)](https://coderabbit.ai)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

MCP-native job search over the **open-jobs** dataset (~1M live roles). A Rust backend exposes search +
optional resume-aware ranking as **MCP tools** (and a small REST/SSE API), with a React UI rendered both
as a standalone web app and as an **MCP App** inside Claude. The intended flow: an agent narrows your
criteria, searches, ranks against your resume with a cheap model (saving cloud tokens), and hands you a
shortlist — while you keep your own tracking spreadsheet.

> **Status: early.** The DuckDB-backed search engine and the MCP server (`search_jobs` + `get_job`, over
> stdio and HTTP) are working — see **[docs/MCP.md](docs/MCP.md)** to run it and connect Claude. Resume-aware
> ranking and the in-conversation React UI come next. Architecture and roadmap: [`docs/DESIGN.md`](docs/DESIGN.md).

## Layout

```
crates/
  joblode-core/    # search / get / rank logic over DuckDB (lib)
  joblode-server/  # axum: REST + SSE + MCP (stdio & HTTP) + MCP App ui:// resource (bin)
web/                # React (Vite, TS) — web UI + MCP App resource
docs/DESIGN.md      # architecture, decisions, phased plan
```

## Develop

The toolchain (Rust, Node, pnpm, DuckDB) is pinned with [flox](https://flox.dev):

```bash
flox activate          # provides cargo, node, pnpm, duckdb

# Rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

# Web
pnpm install
pnpm turbo run lint typecheck test build
```

CI runs all of the above on every push and pull request.

## Use it

Place the open-jobs dataset at the repo root as `open-jobs.parquet` (the default), build, and run:

```bash
cargo build -p joblode-server --release
./target/release/joblode-server stdio   # for Claude; or `http` for an HTTP MCP client
```

Full instructions — getting the data, the transports, and wiring it into Claude Code / Claude Desktop —
are in **[docs/MCP.md](docs/MCP.md)**.

## License

Code: [MIT](LICENSE). The open-jobs dataset itself is released separately under CC0.
