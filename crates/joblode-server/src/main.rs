//! joblode-server — serves the joblode MCP tools over stdio (local clients like
//! Claude Desktop/Code) and streamable HTTP (mounted at `/mcp`).
//!
//! Phase 2: MCP `search_jobs` + `get_job` only. The REST/SSE API and the MCP App
//! `ui://` resource arrive in later phases; see `docs/DESIGN.md`.
//!
//! Usage: `joblode-server [stdio|http]` (default `stdio`). The parquet path comes
//! from `JOBLODE_PARQUET` (default `open-jobs.parquet`) and, for HTTP, the bind
//! address from `JOBLODE_HTTP_ADDR` (default `127.0.0.1:8000`).

mod mcp;

use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use joblode_core::JobStore;
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

    if mode == "stdio" {
        serve_stdio(store).await
    } else {
        serve_http(store).await
    }
}

async fn serve_stdio(store: Arc<Mutex<JobStore>>) -> Result<()> {
    let service = JobServer::new(store).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

async fn serve_http(store: Arc<Mutex<JobStore>>) -> Result<()> {
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

    let service = StreamableHttpService::new(
        move || Ok(JobServer::new(store.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(cancellation.child_token()),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    eprintln!("joblode-server MCP on http://{addr}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancellation.cancel();
        })
        .await?;
    Ok(())
}
