//! The MCP App `ui://` resource (DESIGN §7, Phase 5). One React build serves two
//! runtimes: the standalone web app (over REST) and this resource — a single
//! self-contained HTML page the host renders in a sandboxed iframe, talking back
//! to our tools over the App bridge (postMessage). See `web/src/api.ts`.
//!
//! We expose the bundle as a standard MCP resource and tag the result-returning
//! tools with `_meta.ui.resourceUri` so a host knows which UI to render. Hosts
//! that don't render MCP Apps still get the structured JSON in every tool result —
//! the data stands alone (DESIGN §4).

use rmcp::model::{Meta, RawResource, Resource, ResourceContents};

/// URI of the MCP App bundle. The `ui://` scheme marks it as an App resource; the
/// path is arbitrary (MCP Apps spec / SEP-1865).
pub const APP_URI: &str = "ui://joblode/app";

/// MIME type for HTML MCP App resources, per the MCP Apps spec — a profiled
/// `text/html` so hosts can distinguish an App bundle from a plain HTML resource.
pub const APP_MIME: &str = "text/html;profile=mcp-app";

/// Env var pointing at the built single-file App HTML; defaults to the
/// `vite-plugin-singlefile` output (`pnpm --filter @joblode/web build` emits it).
const APP_HTML_ENV: &str = "JOBLODE_APP_HTML";
/// Resolved at compile time relative to this crate so the bundle is found
/// regardless of the process CWD (a host may launch us from anywhere).
const DEFAULT_APP_HTML: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist-app/index.html");

/// Shown when the App bundle hasn't been built yet, so the server still serves a
/// valid (if inert) resource instead of erroring. The real UI replaces this once
/// `web/dist-app/index.html` exists.
const PLACEHOLDER_HTML: &str = "<!doctype html><meta charset=\"utf-8\"><title>joblode</title>\
<body><p>joblode UI bundle not built. Run <code>pnpm --filter @joblode/web build</code>.</p>";

/// Loads the App bundle HTML: the file at `JOBLODE_APP_HTML` (default
/// `web/dist-app/index.html`), or a placeholder when it isn't built. Read on each
/// resource request — a host fetches the UI rarely (once per render), so the IO is
/// negligible and a rebuilt bundle is picked up without a restart.
#[must_use]
pub fn html() -> String {
    let path = std::env::var(APP_HTML_ENV).unwrap_or_else(|_| DEFAULT_APP_HTML.to_string());
    std::fs::read_to_string(&path).unwrap_or_else(|_| PLACEHOLDER_HTML.to_string())
}

/// The single resource we advertise in `resources/list`.
#[must_use]
pub fn resource() -> Resource {
    let mut raw = RawResource::new(APP_URI, "joblode");
    raw.title = Some("joblode results".to_string());
    raw.description = Some("Interactive results table for joblode searches.".to_string());
    raw.mime_type = Some(APP_MIME.to_string());
    Resource::new(raw, None)
}

/// The `resources/read` payload for [`APP_URI`]: the bundle HTML under the App MIME.
#[must_use]
pub fn contents() -> ResourceContents {
    ResourceContents::text(html(), APP_URI).with_mime_type(APP_MIME)
}

/// `_meta.ui.resourceUri` linking a tool's result to the App UI (MCP Apps spec).
/// Attached to the result-returning tools so a host renders the table.
#[must_use]
pub fn tool_meta() -> Meta {
    let mut map = serde_json::Map::new();
    map.insert(
        "ui".to_string(),
        serde_json::json!({ "resourceUri": APP_URI }),
    );
    Meta(map)
}
