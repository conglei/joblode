# jobscout

[![CI](https://github.com/conglei/jobscout/actions/workflows/ci.yml/badge.svg)](https://github.com/conglei/jobscout/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/conglei/jobscout/branch/main/graph/badge.svg)](https://codecov.io/gh/conglei/jobscout)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/conglei/jobscout/badge)](https://scorecard.dev/viewer/?uri=github.com/conglei/jobscout)
[![CodeRabbit](https://img.shields.io/coderabbit/prs/github/conglei/jobscout?labelColor=171717&color=FF570A&label=CodeRabbit)](https://coderabbit.ai)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

MCP-native job search over the **open-jobs** dataset (~1M live roles). A Rust backend exposes search +
optional resume-aware ranking as **MCP tools** (and a small REST/SSE API), with a React UI rendered both
as a standalone web app and as an **MCP App** inside Claude. The intended flow: an agent narrows your
criteria, searches, ranks against your resume with a cheap model (saving cloud tokens), and hands you a
shortlist — while you keep your own tracking spreadsheet.

> **Status: early.** This is the Phase 0 skeleton. The architecture and roadmap live in
> [`docs/DESIGN.md`](docs/DESIGN.md). The query/rank engine (DuckDB + Gemini) lands in the next phases.

## Layout

```
crates/
  jobscout-core/    # search / get / rank logic over DuckDB (lib)
  jobscout-server/  # axum: REST + SSE + MCP (stdio & HTTP) + MCP App ui:// resource (bin)
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

## License

Code: [MIT](LICENSE). The open-jobs dataset itself is released separately under CC0.
