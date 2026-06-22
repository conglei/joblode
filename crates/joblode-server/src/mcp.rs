//! The joblode MCP server: `search` (filters + optional semantic query), `get_job`,
//! and `rank_jobs` tools over a shared [`JobStore`] (plus optional embed/cheap-model
//! clients). Every tool returns structured JSON; the result-returning tools also
//! carry `_meta.ui.resourceUri` and the server serves the matching `ui://` MCP App
//! resource (the interactive table) — see [`crate::app_ui`] and `docs/DESIGN.md` §7.

use std::sync::{Arc, Mutex};

use joblode_core::{Job, JobStore};
use joblode_rank::{EmbedClient, ModelClient};
use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResult, Implementation, ListResourcesResult,
        ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult,
        ServerCapabilities, ServerInfo,
    },
    schemars,
    service::RequestContext,
    tool, tool_router, ErrorData, Json, RoleServer, ServerHandler,
};
use serde::Deserialize;

use crate::app_ui;
use crate::dto::{RankParams, RankResults, SearchParams, SearchResults};
use crate::ranking::{self, RankError};
use crate::search;

/// Tools whose results render in the MCP App table; tagged with `_meta.ui` so a
/// host fetches and renders the `ui://` bundle. `get_job` is detail-only (the
/// drawer is reached from the table), so it carries no UI link.
const UI_TOOLS: &[&str] = &["search", "rank_jobs"];

/// Identifies one role for [`JobServer::get_job`].
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetJobParams {
    /// Dataset identifier of the role to fetch.
    pub id: String,
}

/// MCP server over one shared, read-only [`JobStore`], with an optional
/// cheap-model client that gates the `match`/`pairwise` ranking methods.
#[derive(Clone)]
pub struct JobServer {
    store: Arc<Mutex<JobStore>>,
    model: Option<Arc<dyn ModelClient>>,
    embed: Option<Arc<dyn EmbedClient>>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for JobServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `ToolRouter` / `dyn` clients are not `Debug`; expose the store and which
        // optional capabilities are configured.
        f.debug_struct("JobServer")
            .field("store", &self.store)
            .field("model_configured", &self.model.is_some())
            .field("embed_configured", &self.embed.is_some())
            .finish_non_exhaustive()
    }
}

#[tool_router]
impl JobServer {
    /// Builds a server backed by `store`. `model` gates the `match`/`pairwise`
    /// ranking methods; `embed` gates the semantic `query`. Both `None` is fine —
    /// search, get_job, and free feedback ranking still work.
    #[must_use]
    pub fn new(
        store: Arc<Mutex<JobStore>>,
        model: Option<Arc<dyn ModelClient>>,
        embed: Option<Arc<dyn EmbedClient>>,
    ) -> Self {
        Self {
            store,
            model,
            embed,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "EXPLORE (refine criteria). One search with two match modes: hard filters (function, level, title, company, city, country, min comp) for keyword/structured match, AND an optional `query` for semantic match against the job description. Use it to surface a page of matches for you and the user to react to, then adjust the filters/query and search again until the criteria are right — then hand off to rank_jobs to order the whole set. With a `query`, rows are ordered by similarity and carry a `score` (needs an embeddings key); without one it's a plain filter page. Call get_job for a role's full description."
    )]
    async fn search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<Json<SearchResults>, ErrorData> {
        search::run(self.store.clone(), self.embed.clone(), params)
            .await
            .map(Json)
            .map_err(|error| match error {
                RankError::BadRequest(message) => ErrorData::invalid_params(message, None),
                RankError::Internal(message) => ErrorData::internal_error(message, None),
            })
    }

    #[tool(
        description = "Fetch one role by id, including its full job description (jd_markdown). Structured fields are LLM extractions; confirm comp, work authorization, and location against jd_markdown."
    )]
    async fn get_job(
        &self,
        Parameters(params): Parameters<GetJobParams>,
    ) -> Result<Json<Job>, ErrorData> {
        let store = self.store.clone();
        let id = params.id;

        let job = tokio::task::spawn_blocking(move || {
            store.lock().expect("store mutex poisoned").get_job(&id)
        })
        .await
        .map_err(|error| ErrorData::internal_error(format!("get_job task failed: {error}"), None))?
        // A genuine query failure is internal; a missing id is the caller's fault.
        .map_err(|error| ErrorData::internal_error(format!("get_job failed: {error}"), None))?
        .ok_or_else(|| ErrorData::invalid_params("no job with that id".to_string(), None))?;

        Ok(Json(job))
    }

    #[tool(
        description = "FINALIZE (order the whole set). Once the search criteria are settled, call rank_jobs with those same hard filters to rank the WHOLE matching set into a shortlist by the user's taste — learned for free from prior feedback (liked/disliked role ids). Fast and keyless; no resume needed. Pass feedback:[{id,label}] and optionally top (default 25; ask for more, e.g. 100, for a final list). Returns {results:[{id, score, why}]}. (Optional cheap-model refine via method 'match'/'pairwise' needs a key + resume and is much slower — rarely needed.)"
    )]
    async fn rank_jobs(
        &self,
        Parameters(params): Parameters<RankParams>,
    ) -> Result<Json<RankResults>, ErrorData> {
        ranking::run(self.store.clone(), self.model.clone(), params)
            .await
            .map(Json)
            .map_err(|error| match error {
                RankError::BadRequest(message) => ErrorData::invalid_params(message, None),
                RankError::Internal(message) => ErrorData::internal_error(message, None),
            })
    }
}

impl ServerHandler for JobServer {
    // Hand-written (rather than `#[tool_handler]`) so `list_tools` can tag the
    // result-returning tools with `_meta.ui.resourceUri`. `call_tool`/`get_tool`
    // mirror the macro's generated bodies exactly.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| {
                if UI_TOOLS.contains(&tool.name.as_ref()) {
                    tool.with_meta(app_ui::tool_meta())
                } else {
                    tool
                }
            })
            .collect();
        Ok(ListToolsResult {
            tools,
            meta: None,
            next_cursor: None,
        })
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        self.tool_router.get(name).cloned().map(|tool| {
            if UI_TOOLS.contains(&name) {
                tool.with_meta(app_ui::tool_meta())
            } else {
                tool
            }
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            resources: vec![app_ui::resource()],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        if request.uri == app_ui::APP_URI {
            Ok(ReadResourceResult::new(vec![app_ui::contents()]))
        } else {
            Err(ErrorData::resource_not_found(
                format!("no resource at {}", request.uri),
                None,
            ))
        }
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
                "joblode exposes the open-jobs dataset. Two stages, kept distinct: (1) EXPLORE with \
                 search — keyword/structured filters plus an optional semantic `query` against the JD; \
                 surface a page of matches for you and the user to react to, refine the filters/query \
                 from that feedback, and repeat until the criteria are right. (2) FINALIZE with \
                 rank_jobs using those same filters — it orders the WHOLE \
                 matching set into a shortlist by the user's taste (learned for free from liked/disliked \
                 role ids; no resume needed). Then get_job for the full description of the few that \
                 matter. Don't dump 40-50 rows per criterion — surface a small batch, learn, rank, \
                 repeat. Structured fields are LLM extractions; confirm comp, work authorization, and \
                 location against jd_markdown. The url is the only apply link — never invent roles."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rmcp::{
        model::CallToolRequestParams, service::RunningService, ClientHandler, RoleClient,
        ServiceExt,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct TestClient;

    impl ClientHandler for TestClient {}

    use crate::ranking::testing::{FavorId, FixedEmbed};

    fn store_at(file: &str) -> Arc<Mutex<JobStore>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(file);
        Arc::new(Mutex::new(
            JobStore::open(path).expect("fixture should open"),
        ))
    }

    fn fixture_store() -> Arc<Mutex<JobStore>> {
        store_at("fixture.parquet")
    }

    /// Fixture with planted `jd_embedding`s, for the ranking tests.
    fn rank_store() -> Arc<Mutex<JobStore>> {
        store_at("rank_fixture.parquet")
    }

    async fn connect() -> RunningService<RoleClient, TestClient> {
        connect_with(JobServer::new(fixture_store(), None, None)).await
    }

    async fn connect_with(server: JobServer) -> RunningService<RoleClient, TestClient> {
        let (server_transport, client_transport) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            let running = server.serve(server_transport).await.expect("server serve");
            let _ = running.waiting().await;
        });
        TestClient
            .serve(client_transport)
            .await
            .expect("client serve")
    }

    fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
        let mut params = CallToolRequestParams::new(name);
        if let Some(object) = arguments.as_object() {
            params = params.with_arguments(object.clone());
        }
        params
    }

    #[tokio::test]
    async fn exposes_search_and_get_tools() {
        let client = connect().await;

        let tools = client.list_all_tools().await.expect("list tools");
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();

        assert!(names.contains(&"search"), "tools: {names:?}");
        assert!(names.contains(&"get_job"), "tools: {names:?}");
        assert!(names.contains(&"rank_jobs"), "tools: {names:?}");
        assert!(!names.contains(&"semantic_search"), "tools: {names:?}");

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn search_returns_total_and_compact_rows() {
        let client = connect().await;

        let result = client
            .call_tool(call(
                "search",
                serde_json::json!({ "cities": ["san francisco"] }),
            ))
            .await
            .expect("search");
        let data = result.structured_content.expect("structured content");

        assert_eq!(data["total"], 3);
        let rows = data["results"].as_array().expect("results array");
        assert_eq!(rows.len(), 3);
        // Compact rows omit the full description — that is get_job's job.
        assert!(rows[0].get("jd_markdown").is_none());

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn search_caps_rows_but_reports_full_total() {
        let client = connect().await;

        let result = client
            .call_tool(call(
                "search",
                serde_json::json!({ "cities": ["san francisco"], "limit": 1 }),
            ))
            .await
            .expect("search");
        let data = result.structured_content.expect("structured content");

        assert_eq!(data["total"], 3);
        assert_eq!(data["results"].as_array().expect("results array").len(), 1);

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn get_job_returns_the_full_description() {
        let client = connect().await;

        let result = client
            .call_tool(call("get_job", serde_json::json!({ "id": "city-direct" })))
            .await
            .expect("get_job");
        let data = result.structured_content.expect("structured content");

        assert_eq!(data["company"], "Acme");
        assert_eq!(data["title"], "Backend Engineer");
        assert_eq!(data["jd_markdown"], "# Backend Engineer");

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn get_job_errors_for_a_missing_id() {
        let client = connect().await;

        let result = client
            .call_tool(call(
                "get_job",
                serde_json::json!({ "id": "not-a-real-job-id" }),
            ))
            .await;

        assert!(result.is_err());

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn rank_jobs_free_method_floats_liked_role_to_the_top() {
        // No model configured; liking the engineering role "city-direct" should
        // pull it to the top via the keyless taste ranker, and rows are compact.
        let client = connect_with(JobServer::new(rank_store(), None, None)).await;

        let result = client
            .call_tool(call(
                "rank_jobs",
                serde_json::json!({
                    "feedback": [{ "id": "city-direct", "label": "liked" }]
                }),
            ))
            .await
            .expect("rank_jobs");
        let data = result.structured_content.expect("structured content");
        let rows = data["results"].as_array().expect("results array");

        assert_eq!(rows[0]["id"], "city-direct");
        assert!(rows[0].get("jd_markdown").is_none(), "rows stay compact");
        assert!(rows[0]["score"].as_f64().unwrap() > rows[1]["score"].as_f64().unwrap());

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn rank_jobs_model_method_errors_without_a_configured_model() {
        // method=match but no model → clean failure, not a silent fallback.
        let client = connect_with(JobServer::new(rank_store(), None, None)).await;

        let result = client
            .call_tool(call(
                "rank_jobs",
                serde_json::json!({ "resume": "engineer", "method": "match" }),
            ))
            .await;

        assert!(result.is_err());

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn rank_jobs_match_method_uses_the_configured_model() {
        // With a model, the match pass reorders by its scores (planted: city-direct=90).
        let server = JobServer::new(rank_store(), Some(Arc::new(FavorId("city-direct"))), None);
        let client = connect_with(server).await;

        let result = client
            .call_tool(call(
                "rank_jobs",
                serde_json::json!({ "resume": "engineer", "method": "match", "top": 3 }),
            ))
            .await
            .expect("rank_jobs");
        let data = result.structured_content.expect("structured content");
        let rows = data["results"].as_array().expect("results array");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["id"], "city-direct");
        assert_eq!(rows[0]["score"], 90.0);
        assert!(rows[0]["why"].as_str().unwrap().contains("planted"));

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn search_with_a_query_orders_by_similarity_with_an_embedder() {
        // The embedder maps any query to the "engineering" direction → city-direct.
        let server = JobServer::new(
            rank_store(),
            None,
            Some(Arc::new(FixedEmbed(vec![1.0, 0.0, 0.0, 0.0]))),
        );
        let client = connect_with(server).await;

        let result = client
            .call_tool(call(
                "search",
                serde_json::json!({ "query": "backend systems engineering" }),
            ))
            .await
            .expect("search");
        let data = result.structured_content.expect("structured content");
        let rows = data["results"].as_array().expect("results array");

        assert_eq!(rows[0]["id"], "city-direct");
        assert!(rows[0]["score"].as_f64().unwrap() > 0.99);

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn result_tools_carry_the_ui_resource_link() {
        // The result-returning tools advertise _meta.ui.resourceUri so a host
        // renders the MCP App; get_job (detail-only) does not.
        let client = connect().await;
        let tools = client.list_all_tools().await.expect("list tools");

        let ui_for = |name: &str| {
            tools
                .iter()
                .find(|t| t.name == name)
                .and_then(|t| t.meta.as_ref())
                .and_then(|m| m.0.get("ui"))
                .and_then(|ui| ui.get("resourceUri"))
                .and_then(|uri| uri.as_str())
                .map(str::to_string)
        };

        assert_eq!(ui_for("search").as_deref(), Some(app_ui::APP_URI));
        assert_eq!(ui_for("rank_jobs").as_deref(), Some(app_ui::APP_URI));
        assert_eq!(ui_for("get_job"), None);

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn lists_the_ui_app_resource() {
        let client = connect().await;

        let resources = client.list_all_resources().await.expect("list resources");
        let app = resources
            .iter()
            .find(|r| r.uri == app_ui::APP_URI)
            .expect("ui:// app resource is advertised");

        assert_eq!(app.mime_type.as_deref(), Some(app_ui::APP_MIME));

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn reads_the_ui_app_resource_as_html() {
        use rmcp::model::{ReadResourceRequestParams, ResourceContents};
        let client = connect().await;

        let result = client
            .read_resource(ReadResourceRequestParams::new(app_ui::APP_URI))
            .await
            .expect("read resource");

        let ResourceContents::TextResourceContents {
            text, mime_type, ..
        } = result.contents.first().expect("one content")
        else {
            panic!("expected text resource contents");
        };
        assert_eq!(mime_type.as_deref(), Some(app_ui::APP_MIME));
        assert!(text.contains("<!doctype html>"), "served HTML: {text}");

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn reading_an_unknown_resource_errors() {
        use rmcp::model::ReadResourceRequestParams;
        let client = connect().await;

        let result = client
            .read_resource(ReadResourceRequestParams::new("ui://joblode/nope"))
            .await;

        assert!(result.is_err());

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn search_with_a_query_errors_without_an_embedder() {
        let client = connect_with(JobServer::new(rank_store(), None, None)).await;

        let result = client
            .call_tool(call("search", serde_json::json!({ "query": "anything" })))
            .await;

        assert!(result.is_err());

        client.cancel().await.ok();
    }
}
