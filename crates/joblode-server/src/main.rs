//! joblode-server — serves the joblode MCP tools over stdio (local clients like
//! Claude Desktop/Code) and, over HTTP, the MCP transport (`/mcp`), the REST API
//! (`/api`), and the React build (static, with an SPA fallback).
//!
//! Tools: `search_jobs`, `get_job`, and `rank_jobs`. The MCP App `ui://` resource
//! arrives in Phase 5; see `docs/DESIGN.md`.
//!
//! Usage: `joblode-server [stdio|http]` (default `stdio`). The parquet path comes
//! from `JOBLODE_PARQUET` (default `open-jobs.parquet`); for HTTP, the bind address
//! from `JOBLODE_HTTP_ADDR` (default `127.0.0.1:8000`) and the web build directory
//! from `JOBLODE_WEB_DIR` (default `web/dist`). Ranking with a cheap model is
//! enabled by `JOBLODE_RANK_PROVIDER=gemini` + `GEMINI_API_KEY` (see
//! `build_model_client`); absent that, the free taste ranking still works.

mod dto;
mod http;
mod mcp;
mod ranking;
mod semantic;

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

use crate::mcp::JobServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Validate the transport before touching the dataset, so a bad invocation
    // fails fast with a clear message instead of a parquet error.
    let mode = std::env::args().nth(1).unwrap_or_else(|| "stdio".into());
    if !matches!(mode.as_str(), "stdio" | "http") {
        bail!("unknown transport '{mode}' (use 'stdio' or 'http')");
    }

    let parquet = std::env::var("JOBLODE_PARQUET").unwrap_or_else(|_| "open-jobs.parquet".into());
    let store = Arc::new(Mutex::new(
        JobStore::open(&parquet).with_context(|| format!("failed to open {parquet}"))?,
    ));
    let model = build_model_client();
    let embed = build_embed_client();

    if mode == "stdio" {
        serve_stdio(store, model, embed).await
    } else {
        serve_http(store, model, embed).await
    }
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
    let service = StreamableHttpService::new(
        move || Ok(JobServer::new(store.clone(), model.clone(), embed.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(cancellation.child_token()),
    );

    // The React build (web UI + the future MCP App ui:// resource); a missing dir
    // simply 404s, so the API still runs before the frontend is built. Unknown
    // paths fall back to index.html for client-side routing.
    let web_dir = std::env::var("JOBLODE_WEB_DIR").unwrap_or_else(|_| "web/dist".into());
    let serve_web = tower_http::services::ServeDir::new(&web_dir).fallback(
        tower_http::services::ServeFile::new(format!("{web_dir}/index.html")),
    );

    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .merge(http::router(api_store, api_model, api_embed))
        .fallback_service(serve_web);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    eprintln!("joblode-server on http://{addr} (REST /api, MCP /mcp)");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancellation.cancel();
        })
        .await?;
    Ok(())
}
