//! The joblode MCP server: `search_jobs`, `get_job`, and `rank_jobs` tools over a
//! shared [`JobStore`] (plus an optional cheap-model client for ranking). Tools
//! return structured JSON only; the `ui://` resource arrives in Phase 5 (see
//! `docs/DESIGN.md`).

use std::sync::{Arc, Mutex};

use joblode_core::{Criteria, Job, JobStore};
use joblode_rank::{Candidate, Method, ModelClient, RankRequest};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ErrorData, Json, ServerHandler,
};
use serde::Deserialize;

use crate::dto::{JobSummary, RankParams, RankResults, SearchParams, SearchResults};

/// Default ranked-shortlist size returned by `rank_jobs`.
const RANK_TOP: usize = 10;

/// How many candidates to draw (by hard filter) before ranking, when ranking
/// from criteria rather than explicit ids.
const RANK_CANDIDATE_LIMIT: usize = 200;

/// How many taste-ordered candidates the `match` pass scores (one model call each).
const REFINE_MATCH: usize = 20;

/// How many the `pairwise` pass compares — smaller, since it is O(n²) calls.
const REFINE_PAIRWISE: usize = 8;

/// Identifies one role for [`JobServer::get_job`].
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetJobParams {
    /// Dataset identifier of the role to fetch.
    pub id: String,
}

/// Candidates plus the feedback embeddings, gathered in one blocking DB pass.
struct Prepared {
    candidates: Vec<Candidate>,
    positives: Vec<Vec<f32>>,
    negatives: Vec<Vec<f32>>,
}

/// MCP server over one shared, read-only [`JobStore`], with an optional
/// cheap-model client that gates the `match`/`pairwise` ranking methods.
#[derive(Clone)]
pub struct JobServer {
    store: Arc<Mutex<JobStore>>,
    model: Option<Arc<dyn ModelClient>>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for JobServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `ToolRouter` and `dyn ModelClient` are not `Debug`; expose the store
        // and whether a model is configured.
        f.debug_struct("JobServer")
            .field("store", &self.store)
            .field("model_configured", &self.model.is_some())
            .finish_non_exhaustive()
    }
}

#[tool_router]
impl JobServer {
    /// Builds a server backed by `store`. `model` is `None` when no cheap-model
    /// key is configured, which disables the `match`/`pairwise` ranking methods
    /// (the free taste ranking still works).
    #[must_use]
    pub fn new(store: Arc<Mutex<JobStore>>, model: Option<Arc<dyn ModelClient>>) -> Self {
        Self {
            store,
            model,
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

    #[tool(
        description = "Rank a candidate set into a compact shortlist to save cloud tokens. Draws candidates by hard filters (or explicit ids), orders them for free against prior feedback (liked/disliked roles), and optionally refines the top with a cheap model (method 'match' or 'pairwise', which need a configured key and a resume). Returns {results:[{id, score, why}]}."
    )]
    async fn rank_jobs(
        &self,
        Parameters(params): Parameters<RankParams>,
    ) -> Result<Json<RankResults>, ErrorData> {
        let method = parse_method(params.method.as_deref())?;
        let top = params.top.unwrap_or(RANK_TOP);
        let refine_k = match method {
            Method::Pairwise => REFINE_PAIRWISE,
            _ => REFINE_MATCH,
        };

        let store = self.store.clone();
        let criteria = params.filter.criteria();
        let candidate_limit = params.filter.limit.unwrap_or(RANK_CANDIDATE_LIMIT);
        let ids = params.ids;
        let feedback = params.feedback;

        // One blocking DB pass: draw candidates and the feedback embeddings.
        let prepared = tokio::task::spawn_blocking(move || {
            let store = store.lock().expect("store mutex poisoned");
            prepare_candidates(&store, &criteria, candidate_limit, &ids, &feedback)
        })
        .await
        .map_err(|error| ErrorData::internal_error(format!("rank task failed: {error}"), None))?
        .map_err(|error| ErrorData::internal_error(format!("rank prep failed: {error}"), None))?;

        let request = RankRequest {
            resume: params.resume.as_deref(),
            candidates: prepared.candidates,
            positives: prepared.positives,
            negatives: prepared.negatives,
            method,
            top,
            refine_k,
        };

        let results = joblode_rank::rank(self.model.as_deref(), request)
            .await
            .map_err(|error| {
                let message = error.to_string();
                // "requires …" = a config/caller problem; anything else is internal.
                if message.contains("requires") {
                    ErrorData::invalid_params(message, None)
                } else {
                    ErrorData::internal_error(message, None)
                }
            })?;

        Ok(Json(RankResults { results }))
    }
}

/// Parses the `method` string into a [`Method`], defaulting to free taste ranking.
fn parse_method(method: Option<&str>) -> Result<Method, ErrorData> {
    match method.map(|m| m.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("free") => Ok(Method::Free),
        Some("match") => Ok(Method::Match),
        Some("pairwise") => Ok(Method::Pairwise),
        Some(other) => Err(ErrorData::invalid_params(
            format!("unknown rank method '{other}' (use 'match', 'pairwise', or omit)"),
            None,
        )),
    }
}

/// Draws the candidate set and resolves feedback ids to embeddings, all under one
/// held store lock. Candidates missing an embedding still rank (taste score 0).
fn prepare_candidates(
    store: &JobStore,
    criteria: &Criteria,
    candidate_limit: usize,
    ids: &[String],
    feedback: &[crate::dto::FeedbackItem],
) -> anyhow::Result<Prepared> {
    let jobs: Vec<Job> = if ids.is_empty() {
        store.search(criteria, candidate_limit)?.0
    } else {
        let mut found = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(job) = store.get_job(id)? {
                found.push(job);
            }
        }
        found
    };

    // Fetch embeddings for candidates and feedback ids together (deduplicated).
    let mut wanted: Vec<String> = jobs.iter().map(|job| job.id.clone()).collect();
    wanted.extend(feedback.iter().map(|item| item.id.clone()));
    wanted.sort();
    wanted.dedup();
    let wanted_refs: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let embeddings = store.embeddings(&wanted_refs)?;

    let candidates = jobs
        .into_iter()
        .map(|job| {
            let embedding = embeddings.get(job.id.as_str()).cloned().unwrap_or_default();
            Candidate {
                id: job.id,
                title: job.title,
                summary: job.role_summary,
                embedding,
            }
        })
        .collect();

    let mut positives = Vec::new();
    let mut negatives = Vec::new();
    for item in feedback {
        if let Some(embedding) = embeddings.get(item.id.as_str()) {
            match item.polarity() {
                Some(true) => positives.push(embedding.clone()),
                Some(false) => negatives.push(embedding.clone()),
                None => {}
            }
        }
    }

    Ok(Prepared {
        candidates,
        positives,
        negatives,
    })
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
                 hard filters, then rank_jobs to reduce it to a compact shortlist (cheaply, against \
                 the user's prior feedback) before reading details, and get_job for a role's full \
                 description. Structured fields are LLM extractions; confirm comp, work authorization, \
                 and location against jd_markdown. The url is the only apply link — never invent roles."
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

    /// Deterministic model for the ranking tests: `match` favors `city-direct`,
    /// `compare` orders by id. No network.
    struct FakeModel;

    #[async_trait::async_trait]
    impl ModelClient for FakeModel {
        async fn match_score(
            &self,
            _resume: &str,
            job: &joblode_rank::JobText,
        ) -> anyhow::Result<joblode_rank::MatchScore> {
            let score = if job.id == "city-direct" { 90.0 } else { 10.0 };
            Ok(joblode_rank::MatchScore {
                score,
                why: format!("planted fit for {}", job.id),
            })
        }

        async fn compare(
            &self,
            _resume: &str,
            a: &joblode_rank::JobText,
            b: &joblode_rank::JobText,
        ) -> anyhow::Result<std::cmp::Ordering> {
            Ok(a.id.cmp(&b.id))
        }
    }

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
        connect_with(JobServer::new(fixture_store(), None)).await
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

        assert!(names.contains(&"search_jobs"), "tools: {names:?}");
        assert!(names.contains(&"get_job"), "tools: {names:?}");
        assert!(names.contains(&"rank_jobs"), "tools: {names:?}");

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

    #[tokio::test]
    async fn rank_jobs_free_method_floats_liked_role_to_the_top() {
        // No model configured; liking the engineering role "city-direct" should
        // pull it to the top via the keyless taste ranker, and rows are compact.
        let client = connect_with(JobServer::new(rank_store(), None)).await;

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
        let client = connect_with(JobServer::new(rank_store(), None)).await;

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
        let server = JobServer::new(rank_store(), Some(Arc::new(FakeModel)));
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
}
