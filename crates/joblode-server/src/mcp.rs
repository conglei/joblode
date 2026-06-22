//! The joblode MCP server: `search_jobs` and `get_job` tools over a shared
//! [`JobStore`]. Tools return structured JSON only; the `ui://` resource arrives
//! in Phase 5 (see `docs/DESIGN.md`).

use std::sync::{Arc, Mutex};

use joblode_core::{Job, JobStore};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ErrorData, Json, ServerHandler,
};
use serde::Deserialize;

use crate::dto::{JobSummary, SearchParams, SearchResults};

/// Identifies one role for [`JobServer::get_job`].
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetJobParams {
    /// Dataset identifier of the role to fetch.
    pub id: String,
}

/// MCP server over one shared, read-only [`JobStore`].
#[derive(Clone)]
pub struct JobServer {
    store: Arc<Mutex<JobStore>>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for JobServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `ToolRouter` is not `Debug`; expose only the backing store.
        f.debug_struct("JobServer")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

#[tool_router]
impl JobServer {
    /// Builds a server backed by `store`.
    #[must_use]
    pub fn new(store: Arc<Mutex<JobStore>>) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Search live roles by hard filters (function, level, title, company, city, country, min comp). Returns a total match count and compact rows; call get_job for a role's full description."
    )]
    async fn search_jobs(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<Json<SearchResults>, ErrorData> {
        let criteria = params.criteria();
        let limit = params.effective_limit();
        let store = self.store.clone();

        let (jobs, total) = tokio::task::spawn_blocking(move || {
            store
                .lock()
                .expect("store mutex poisoned")
                .search(&criteria, limit)
        })
        .await
        .map_err(|error| ErrorData::internal_error(format!("search task failed: {error}"), None))?
        .map_err(|error| ErrorData::internal_error(format!("search failed: {error}"), None))?;

        let results = jobs.iter().map(JobSummary::from).collect();
        Ok(Json(SearchResults { total, results }))
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
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for JobServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "joblode exposes the open-jobs dataset. Use search_jobs to draw a candidate set with \
                 hard filters, then get_job for a role's full description. Structured fields are LLM \
                 extractions; confirm comp, work authorization, and location against jd_markdown. The \
                 url is the only apply link — never invent roles."
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

    fn fixture_store() -> Arc<Mutex<JobStore>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixture.parquet");
        Arc::new(Mutex::new(
            JobStore::open(path).expect("fixture should open"),
        ))
    }

    async fn connect() -> RunningService<RoleClient, TestClient> {
        let server = JobServer::new(fixture_store());
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

        assert!(names.contains(&"search_jobs"), "tools: {names:?}");
        assert!(names.contains(&"get_job"), "tools: {names:?}");

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn search_jobs_returns_total_and_compact_rows() {
        let client = connect().await;

        let result = client
            .call_tool(call(
                "search_jobs",
                serde_json::json!({ "cities": ["san francisco"] }),
            ))
            .await
            .expect("search_jobs");
        let data = result.structured_content.expect("structured content");

        assert_eq!(data["total"], 3);
        let rows = data["results"].as_array().expect("results array");
        assert_eq!(rows.len(), 3);
        // Compact rows omit the full description — that is get_job's job.
        assert!(rows[0].get("jd_markdown").is_none());

        client.cancel().await.ok();
    }

    #[tokio::test]
    async fn search_jobs_caps_rows_but_reports_full_total() {
        let client = connect().await;

        let result = client
            .call_tool(call(
                "search_jobs",
                serde_json::json!({ "cities": ["san francisco"], "limit": 1 }),
            ))
            .await
            .expect("search_jobs");
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
}
