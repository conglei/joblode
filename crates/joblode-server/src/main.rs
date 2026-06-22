//! joblode-server — serves the joblode MCP tools over stdio (local clients like
//! Claude Desktop/Code) and, over HTTP, the MCP transport (`/mcp`), the REST API
//! (`/api`), and the React build (static, with an SPA fallback).
//!
//! Tools: `search` (filters + optional semantic query), `get_job`, `rank_jobs`. Over HTTP
//! it also serves the MCP App `ui://` bundle (`web/dist-app`) and the standalone
//! React build (`web/dist`); see `docs/DESIGN.md` §7.
//!
//! Config comes from the environment (a gitignored `.env` is loaded at startup;
//! see `.env.example`). `joblode-server [stdio|http]` (default `stdio`); parquet
//! from `JOBLODE_PARQUET` (default `open-jobs.parquet`); for HTTP, bind from
//! `JOBLODE_HTTP_ADDR` (default `127.0.0.1:8000`) and the web build from
//! `JOBLODE_WEB_DIR` (default `web/dist`). Ranking
//! (`JOBLODE_RANK_PROVIDER=gemini`, `GEMINI_API_KEY`) and semantic search
//! (`JOBLODE_EMBED_PROVIDER=openai`, `OPENAI_API_KEY`) are config-gated; absent
//! their keys, free search/ranking work.

mod app_ui;
mod dto;
mod http;
mod mcp;
mod ranking;
mod search;
mod searchapi;

use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use joblode_core::JobStore;
use joblode_rank::{EmbedClient, GeminiClient, ModelClient, OpenAiEmbedder};
use rmcp::transport::{
    stdio,
    streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
};
use rmcp::ServiceExt;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tower_http::LatencyUnit;
use tracing::Level;

use crate::mcp::JobServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Load a gitignored `.env` (if present) before reading any config, so keys
    // like GEMINI_API_KEY / OPENAI_API_KEY needn't be re-exported each run. Real
    // environment variables take precedence; a missing file is fine.
    let _ = dotenvy::dotenv();
    init_tracing();

    // Validate the command before touching the dataset, so a bad invocation
    // fails fast with a clear message instead of a parquet error.
    let mode = std::env::args().nth(1).unwrap_or_else(|| "stdio".into());
    if !matches!(mode.as_str(), "stdio" | "http" | "build-sidecar") {
        bail!("unknown command '{mode}' (use 'stdio', 'http', or 'build-sidecar')");
    }

    let parquet = std::env::var("JOBLODE_PARQUET").unwrap_or_else(|_| "open-jobs.parquet".into());

    // One-shot maintenance command: build the compact embedding sidecar and exit.
    if mode == "build-sidecar" {
        return build_sidecar(&parquet);
    }

    let mut store =
        JobStore::open(&parquet).with_context(|| format!("failed to open {parquet}"))?;
    attach_sidecar(&mut store, &parquet);
    let store = Arc::new(Mutex::new(store));
    let model = build_model_client();
    let embed = build_embed_client();
    let searchapi = build_searchapi_client();
    tracing::info!(
        transport = %mode,
        parquet = %parquet,
        ranking = model.is_some(),
        embeddings = embed.is_some(),
        searchapi = searchapi.is_some(),
        "starting joblode-server"
    );

    if mode == "stdio" {
        serve_stdio(store, model, embed).await
    } else {
        serve_http(store, model, embed).await
    }
}

/// Path of the embedding sidecar: `JOBLODE_EMBED_SIDECAR`, or `<parquet>.emb.parquet`.
fn sidecar_path(parquet: &str) -> String {
    std::env::var("JOBLODE_EMBED_SIDECAR").unwrap_or_else(|_| format!("{parquet}.emb.parquet"))
}

/// Builds the compact embedding sidecar (truncated `jd_embedding`) next to the
/// dataset, for fast semantic search. Run once after each data refresh. Dimension
/// from `JOBLODE_EMBED_DIM` (default 256).
fn build_sidecar(parquet: &str) -> Result<()> {
    let out = sidecar_path(parquet);
    let target_dim = std::env::var("JOBLODE_EMBED_DIM")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(256);
    let store = JobStore::open(parquet).with_context(|| format!("failed to open {parquet}"))?;
    tracing::info!(parquet, out = %out, target_dim, "building embedding sidecar");
    let dim = store
        .build_embedding_sidecar(&out, target_dim)
        .with_context(|| format!("failed to build sidecar at {out}"))?;
    tracing::info!(out = %out, dim, "built embedding sidecar");
    Ok(())
}

/// Attaches the sidecar to `store` if present, enabling the fast semantic path.
/// A missing or unreadable sidecar is non-fatal — semantic search falls back to
/// scanning the full embeddings (slow); we just warn how to build one.
fn attach_sidecar(store: &mut JobStore, parquet: &str) {
    let path = sidecar_path(parquet);
    if !std::path::Path::new(&path).exists() {
        tracing::warn!(
            expected = %path,
            "no embedding sidecar; semantic search will scan full embeddings (slow). \
             Build one with: joblode-server build-sidecar"
        );
        return;
    }
    match store.attach_sidecar(&path) {
        Ok(()) => {
            tracing::info!(sidecar = %path, "attached embedding sidecar (fast semantic search)")
        }
        Err(error) => tracing::warn!(
            sidecar = %path,
            %error,
            "failed to attach embedding sidecar; semantic search will use the slow path"
        ),
    }
}

/// Initialises the `tracing` subscriber. Events go to **stderr** — stdout carries
/// the MCP stdio protocol, so logging there would corrupt it — and are filtered by
/// `RUST_LOG`, defaulting to `info` with our crates at `debug`. Set e.g.
/// `RUST_LOG=joblode_server=debug,tower_http=debug` for verbose request traces.
fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,joblode_server=debug,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();
}

/// Builds the query-embedding client for semantic search from env, or `None` when
/// it's unconfigured. Enabled when `JOBLODE_EMBED_PROVIDER=openai` and the key env
/// var (default `OPENAI_API_KEY`) is set; model/base URL fall back to defaults
/// (`text-embedding-3-small`, matching the dataset's vectors).
fn build_embed_client() -> Option<Arc<dyn EmbedClient>> {
    let provider = std::env::var("JOBLODE_EMBED_PROVIDER").unwrap_or_default();
    if !provider.eq_ignore_ascii_case("openai") {
        return None;
    }
    let key_var =
        std::env::var("JOBLODE_EMBED_API_KEY_ENV").unwrap_or_else(|_| "OPENAI_API_KEY".into());
    let api_key = std::env::var(&key_var).ok().filter(|key| !key.is_empty())?;
    let base_url = std::env::var("JOBLODE_EMBED_BASE_URL").unwrap_or_default();
    let model = std::env::var("JOBLODE_EMBED_MODEL").unwrap_or_default();

    Some(Arc::new(OpenAiEmbedder::new(api_key, base_url, model)))
}

/// Builds the SearchAPI (Google Jobs) client from env, or `None` when unconfigured.
/// Enabled when `SEARCHAPI_API_KEY` is set (override the var name with
/// `JOBLODE_SEARCHAPI_API_KEY_ENV`); base URL and the per-search page cap fall back
/// to defaults (`JOBLODE_SEARCHAPI_BASE_URL`, `JOBLODE_SEARCHAPI_MAX_PAGES`).
fn build_searchapi_client() -> Option<searchapi::SearchApiClient> {
    let key_var = std::env::var("JOBLODE_SEARCHAPI_API_KEY_ENV")
        .unwrap_or_else(|_| "SEARCHAPI_API_KEY".into());
    let api_key = std::env::var(&key_var).ok().filter(|key| !key.is_empty())?;
    let base_url = std::env::var("JOBLODE_SEARCHAPI_BASE_URL").unwrap_or_default();
    let max_pages = std::env::var("JOBLODE_SEARCHAPI_MAX_PAGES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5);
    Some(searchapi::SearchApiClient::new(
        api_key, base_url, max_pages,
    ))
}

/// Builds the cheap-model ranking client from env, or `None` when ranking is
/// unconfigured (the free taste ranking still works). Enabled when
/// `JOBLODE_RANK_PROVIDER=gemini` and the key env var (default `GEMINI_API_KEY`)
/// is set; models and base URL fall back to sensible defaults.
fn build_model_client() -> Option<Arc<dyn ModelClient>> {
    let provider = std::env::var("JOBLODE_RANK_PROVIDER").unwrap_or_default();
    if !provider.eq_ignore_ascii_case("gemini") {
        return None;
    }
    let key_var =
        std::env::var("JOBLODE_RANK_API_KEY_ENV").unwrap_or_else(|_| "GEMINI_API_KEY".into());
    let api_key = std::env::var(&key_var).ok().filter(|key| !key.is_empty())?;
    let base_url = std::env::var("JOBLODE_RANK_BASE_URL").unwrap_or_default();
    let match_model =
        std::env::var("JOBLODE_RANK_MATCH_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".into());
    let pair_model =
        std::env::var("JOBLODE_RANK_PAIR_MODEL").unwrap_or_else(|_| "gemini-2.5-flash-lite".into());

    Some(Arc::new(GeminiClient::new(
        api_key,
        base_url,
        match_model,
        pair_model,
    )))
}

async fn serve_stdio(
    store: Arc<Mutex<JobStore>>,
    model: Option<Arc<dyn ModelClient>>,
    embed: Option<Arc<dyn EmbedClient>>,
) -> Result<()> {
    let service = JobServer::new(store, model, embed).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

async fn serve_http(
    store: Arc<Mutex<JobStore>>,
    model: Option<Arc<dyn ModelClient>>,
    embed: Option<Arc<dyn EmbedClient>>,
) -> Result<()> {
    let addr_str = std::env::var("JOBLODE_HTTP_ADDR").unwrap_or_else(|_| "127.0.0.1:8000".into());
    // The server is local-only by design (see DESIGN §13); refuse to bind a
    // non-loopback address so JOBLODE_HTTP_ADDR can set the port but not expose us.
    let addr: std::net::SocketAddr = addr_str
        .parse()
        .with_context(|| format!("JOBLODE_HTTP_ADDR must be ip:port, got '{addr_str}'"))?;
    if !addr.ip().is_loopback() {
        bail!("JOBLODE_HTTP_ADDR must be a loopback address (got '{addr_str}'); the server is local-only");
    }
    let cancellation = tokio_util::sync::CancellationToken::new();

    // The MCP service closure takes ownership of `store`/`model`; the REST router
    // needs its own handles to the same shared store and model.
    let api_store = store.clone();
    let api_model = model.clone();
    let api_embed = embed.clone();
    // rmcp validates the inbound `Host` header against loopback by default (DNS
    // rebinding protection). A tunnelled deployment (e.g. cloudflared for a Claude
    // custom connector) arrives with a public Host, so allow it via
    // `JOBLODE_ALLOWED_HOSTS` (comma-separated; `*` allows any). The server still
    // *binds* loopback — only the local tunnel reaches it — so this stays safe.
    let mcp_config =
        StreamableHttpServerConfig::default().with_cancellation_token(cancellation.child_token());
    let mcp_config = match std::env::var("JOBLODE_ALLOWED_HOSTS") {
        Ok(value) if value.trim() == "*" => mcp_config.disable_allowed_hosts(),
        Ok(value) => {
            let mut hosts: Vec<String> =
                ["localhost", "127.0.0.1", "::1"].map(String::from).to_vec();
            hosts.extend(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|host| !host.is_empty())
                    .map(String::from),
            );
            mcp_config.with_allowed_hosts(hosts)
        }
        Err(_) => mcp_config,
    };
    let service = StreamableHttpService::new(
        move || Ok(JobServer::new(store.clone(), model.clone(), embed.clone())),
        LocalSessionManager::default().into(),
        mcp_config,
    );

    // The React build (web UI + the future MCP App ui:// resource); a missing dir
    // simply 404s, so the API still runs before the frontend is built. Unknown
    // paths fall back to index.html for client-side routing.
    let web_dir = std::env::var("JOBLODE_WEB_DIR").unwrap_or_else(|_| "web/dist".into());
    let serve_web = tower_http::services::ServeDir::new(&web_dir).fallback(
        tower_http::services::ServeFile::new(format!("{web_dir}/index.html")),
    );

    // Log every HTTP request as a span (method + path) and its response with
    // status + latency in ms — this is what surfaces, e.g., a slow /api/semantic.
    let trace = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .latency_unit(LatencyUnit::Millis),
        );

    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .merge(http::router(api_store, api_model, api_embed))
        // joblode is local and unauthenticated (DESIGN §9). Answer OAuth-discovery
        // probes (RFC 8414 / 9728) with a clean 404 so clients treat us as
        // no-auth, instead of letting the SPA fallback return index.html (HTML 200)
        // — which derails connector auto-registration (e.g. claude.ai connectors).
        .route(
            "/.well-known/{*path}",
            axum::routing::any(|| async { axum::http::StatusCode::NOT_FOUND }),
        )
        .fallback_service(serve_web)
        .layer(trace);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "joblode-server listening (REST /api, MCP /mcp)");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancellation.cancel();
        })
        .await?;
    Ok(())
}
