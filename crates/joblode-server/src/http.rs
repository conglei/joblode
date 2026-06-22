//! The REST face of joblode: `POST /api/search` and `GET /api/job/{id}` over a
//! shared [`JobStore`]. Same wire shapes as the MCP tools (see `dto`); this is the
//! adapter the standalone web UI talks to. See `docs/DESIGN.md` §7.

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use joblode_core::{Job, JobStore};

use crate::dto::{JobSummary, SearchParams, SearchResults};

/// Shared, read-only state for the API handlers.
#[derive(Clone)]
struct ApiState {
    store: Arc<Mutex<JobStore>>,
}

/// Builds the `/api/*` router backed by `store`.
pub fn router(store: Arc<Mutex<JobStore>>) -> Router {
    Router::new()
        .route("/api/search", post(search))
        .route("/api/job/{id}", get(job))
        .with_state(ApiState { store })
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

    fn app() -> Router {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixture.parquet");
        let store = Arc::new(Mutex::new(
            JobStore::open(path).expect("fixture should open"),
        ));
        router(store)
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
}
