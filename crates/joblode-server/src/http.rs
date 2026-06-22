//! The REST face of joblode: `POST /api/search`, `GET /api/job/{id}`, and
//! `POST /api/rank` over a shared [`JobStore`] (+ optional ranking model). Same
//! wire shapes as the MCP tools (see `dto`); this is the adapter the standalone
//! web UI talks to. See `docs/DESIGN.md` §7.

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use joblode_core::{Job, JobStore};
use joblode_rank::{EmbedClient, ModelClient};

use crate::dto::{
    JobSummary, RankParams, RankResults, SearchParams, SearchResults, SemanticParams,
    SemanticResults,
};
use crate::ranking::{self, RankError};
use crate::semantic;

/// Shared, read-only state for the API handlers.
#[derive(Clone)]
struct ApiState {
    store: Arc<Mutex<JobStore>>,
    model: Option<Arc<dyn ModelClient>>,
    embed: Option<Arc<dyn EmbedClient>>,
}

/// Builds the `/api/*` router backed by `store`, the optional ranking `model`,
/// and the optional `embed` client for semantic search.
pub fn router(
    store: Arc<Mutex<JobStore>>,
    model: Option<Arc<dyn ModelClient>>,
    embed: Option<Arc<dyn EmbedClient>>,
) -> Router {
    Router::new()
        .route("/api/search", post(search))
        .route("/api/job/{id}", get(job))
        .route("/api/rank", post(rank))
        .route("/api/semantic", post(semantic_search))
        .with_state(ApiState {
            store,
            model,
            embed,
        })
}

/// `POST /api/search` — hard-filter the corpus, returning the full match count
/// plus a capped page of compact rows.
async fn search(
    State(state): State<ApiState>,
    Json(params): Json<SearchParams>,
) -> Result<Json<SearchResults>, (StatusCode, String)> {
    let criteria = params.criteria();
    let limit = params.effective_limit();
    let store = state.store.clone();

    let (jobs, total) = tokio::task::spawn_blocking(move || {
        store
            .lock()
            .expect("store mutex poisoned")
            .search(&criteria, limit)
    })
    .await
    .map_err(|error| internal("search task", error))?
    .map_err(|error| internal("search", error))?;

    let results = jobs.iter().map(JobSummary::from).collect();
    Ok(Json(SearchResults { total, results }))
}

/// `GET /api/job/{id}` — the full record, including `jd_markdown`. 404 if unknown.
async fn job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, (StatusCode, String)> {
    let store = state.store.clone();

    let job = tokio::task::spawn_blocking(move || {
        store.lock().expect("store mutex poisoned").get_job(&id)
    })
    .await
    .map_err(|error| internal("get_job task", error))?
    .map_err(|error| internal("get_job", error))?
    .ok_or((StatusCode::NOT_FOUND, "no job with that id".to_string()))?;

    Ok(Json(job))
}

/// `POST /api/rank` — reduce a candidate set to a compact, ordered shortlist. The
/// free taste ranking needs no key; `method` "match"/"pairwise" require a
/// configured model and a resume (400 otherwise).
async fn rank(
    State(state): State<ApiState>,
    Json(params): Json<RankParams>,
) -> Result<Json<RankResults>, (StatusCode, String)> {
    ranking::run(state.store.clone(), state.model.clone(), params)
        .await
        .map(Json)
        .map_err(|error| match error {
            RankError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            RankError::Internal(detail) => internal("rank", detail),
        })
}

/// `POST /api/semantic` — free-text semantic search over role embeddings. 400 if
/// the query is empty or no embeddings model is configured.
async fn semantic_search(
    State(state): State<ApiState>,
    Json(params): Json<SemanticParams>,
) -> Result<Json<SemanticResults>, (StatusCode, String)> {
    semantic::run(state.store.clone(), state.embed.clone(), params)
        .await
        .map(Json)
        .map_err(|error| match error {
            RankError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            RankError::Internal(detail) => internal("semantic", detail),
        })
}

/// Logs the real failure server-side and returns an opaque 500, so DuckDB/query
/// internals never travel over the API surface.
fn internal(context: &str, detail: impl std::fmt::Display) -> (StatusCode, String) {
    eprintln!("joblode-server: {context} failed: {detail}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::*;

    fn store_at(file: &str) -> Arc<Mutex<JobStore>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(file);
        Arc::new(Mutex::new(
            JobStore::open(path).expect("fixture should open"),
        ))
    }

    fn app() -> Router {
        router(store_at("fixture.parquet"), None, None)
    }

    /// Router over the embedding fixture, with an optional ranking model.
    fn rank_app(model: Option<Arc<dyn ModelClient>>) -> Router {
        router(store_at("rank_fixture.parquet"), model, None)
    }

    /// Router over the embedding fixture, with an optional embeddings client.
    fn semantic_app(embed: Option<Arc<dyn EmbedClient>>) -> Router {
        router(store_at("rank_fixture.parquet"), None, embed)
    }

    fn post_json(uri: &str, payload: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("build request")
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("body is json")
    }

    fn post_search(payload: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/search")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("build request")
    }

    #[tokio::test]
    async fn search_returns_total_and_compact_rows() {
        let response = app()
            .oneshot(post_search(
                serde_json::json!({ "cities": ["san francisco"] }),
            ))
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::OK);

        let data = body_json(response).await;
        assert_eq!(data["total"], 3);
        let rows = data["results"].as_array().expect("results array");
        assert_eq!(rows.len(), 3);
        // Compact rows omit the full description — that is get_job's job.
        assert!(rows[0].get("jd_markdown").is_none());
    }

    #[tokio::test]
    async fn search_caps_rows_but_reports_full_total() {
        let response = app()
            .oneshot(post_search(
                serde_json::json!({ "cities": ["san francisco"], "limit": 1 }),
            ))
            .await
            .expect("request");

        let data = body_json(response).await;
        assert_eq!(data["total"], 3);
        assert_eq!(data["results"].as_array().expect("results array").len(), 1);
    }

    #[tokio::test]
    async fn search_with_empty_body_matches_everything() {
        // An empty object is a valid SearchParams (every field defaults).
        let response = app()
            .oneshot(post_search(serde_json::json!({})))
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_job_returns_the_full_description() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/api/job/city-direct")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::OK);

        let data = body_json(response).await;
        assert_eq!(data["company"], "Acme");
        assert_eq!(data["title"], "Backend Engineer");
        assert_eq!(data["jd_markdown"], "# Backend Engineer");
    }

    #[tokio::test]
    async fn get_job_404s_for_a_missing_id() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/api/job/not-a-real-job-id")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rank_free_method_floats_liked_role_to_the_top() {
        // No model needed: liking "city-direct" pulls it to the top for free.
        let response = rank_app(None)
            .oneshot(post_json(
                "/api/rank",
                serde_json::json!({ "feedback": [{ "id": "city-direct", "label": "liked" }] }),
            ))
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::OK);

        let data = body_json(response).await;
        let rows = data["results"].as_array().expect("results array");
        assert_eq!(rows[0]["id"], "city-direct");
    }

    #[tokio::test]
    async fn rank_model_method_400s_without_a_configured_model() {
        let response = rank_app(None)
            .oneshot(post_json(
                "/api/rank",
                serde_json::json!({ "resume": "engineer", "method": "match" }),
            ))
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn semantic_search_orders_by_similarity() {
        let embed = Arc::new(crate::ranking::testing::FixedEmbed(vec![
            1.0, 0.0, 0.0, 0.0,
        ]));
        let response = semantic_app(Some(embed))
            .oneshot(post_json(
                "/api/semantic",
                serde_json::json!({ "query": "backend engineering" }),
            ))
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::OK);

        let data = body_json(response).await;
        let rows = data["results"].as_array().expect("results array");
        assert_eq!(rows[0]["id"], "city-direct");
    }

    #[tokio::test]
    async fn semantic_search_400s_without_an_embedder() {
        let response = semantic_app(None)
            .oneshot(post_json(
                "/api/semantic",
                serde_json::json!({ "query": "backend engineering" }),
            ))
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rank_match_method_uses_the_configured_model() {
        let model = Arc::new(crate::ranking::testing::FavorId("city-direct"));
        let response = rank_app(Some(model))
            .oneshot(post_json(
                "/api/rank",
                serde_json::json!({ "resume": "engineer", "method": "match", "top": 3 }),
            ))
            .await
            .expect("request");
        assert_eq!(response.status(), StatusCode::OK);

        let data = body_json(response).await;
        let rows = data["results"].as_array().expect("results array");
        assert_eq!(rows[0]["id"], "city-direct");
        assert_eq!(rows[0]["score"], 90.0);
    }
}
