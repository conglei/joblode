//! Unified search orchestration behind the `search` MCP tool and `/api/search`.
//! A plain keyword/structured-filter search, or — when a semantic `query` is given
//! — rows ordered by cosine similarity of the query to each role (best of title /
//! JD / alternate titles), under the same hard filters. The query path needs a
//! configured embeddings model; the filter-only path needs no key. Reuses
//! [`crate::ranking::RankError`] so each face maps it the same way.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use joblode_core::JobStore;
use joblode_rank::EmbedClient;

use crate::dto::{JobSummary, SearchHit, SearchParams, SearchResults};
use crate::ranking::RankError;

/// Runs one search: filter-only, or semantic when `params.query` is set.
///
/// # Errors
///
/// [`RankError::BadRequest`] when a `query` is given but no embeddings model is
/// configured; [`RankError::Internal`] for an embedding-call or query failure.
pub async fn run(
    store: Arc<Mutex<JobStore>>,
    embed: Option<Arc<dyn EmbedClient>>,
    params: SearchParams,
) -> Result<SearchResults, RankError> {
    let limit = params.effective_limit();
    let criteria = params.criteria();

    let Some(query) = params.semantic_query() else {
        // Keyword / structured filter search — no embeddings needed.
        let (jobs, total) = tokio::task::spawn_blocking(move || {
            store
                .lock()
                .expect("store mutex poisoned")
                .search(&criteria, limit)
        })
        .await
        .map_err(|error| RankError::Internal(format!("search task failed: {error}")))?
        .map_err(|error| RankError::Internal(format!("search failed: {error}")))?;

        let results = jobs
            .iter()
            .map(|job| SearchHit {
                summary: JobSummary::from(job),
                score: None,
            })
            .collect();
        return Ok(SearchResults { total, results });
    };

    // Semantic search: embed the query, then cosine-rank the filtered corpus.
    let query = query.to_owned();
    let embed = embed.ok_or_else(|| {
        RankError::BadRequest(
            "a semantic query requires a configured embeddings model; none is set".to_owned(),
        )
    })?;

    let embed_started = Instant::now();
    let vector = embed
        .embed(&query)
        .await
        .map_err(|error| RankError::Internal(format!("embedding failed: {error}")))?;
    let embed_ms = embed_started.elapsed().as_millis();

    let filtered = !criteria.is_empty();
    let index_dim = store
        .lock()
        .expect("store mutex poisoned")
        .semantic_index_dim();
    tracing::info!(
        filtered,
        sidecar = index_dim.is_some(),
        scan_dim = index_dim.unwrap_or(vector.len()),
        limit,
        "search: semantic cosine scan"
    );

    let query_started = Instant::now();
    // `total` is the size of the hard-filtered set (the query only ranks within it),
    // so the caller knows how many roles match even though `limit` caps the rows.
    let (hits, total) = tokio::task::spawn_blocking(move || {
        let store = store.lock().expect("store mutex poisoned");
        let total = store.count(&criteria)?;
        let hits = store.semantic_search(&vector, &criteria, limit)?;
        anyhow::Ok((hits, total))
    })
    .await
    .map_err(|error| RankError::Internal(format!("search task failed: {error}")))?
    .map_err(|error| RankError::Internal(format!("semantic search failed: {error}")))?;
    let query_ms = query_started.elapsed().as_millis();

    let results: Vec<SearchHit> = hits
        .into_iter()
        .map(|(job, score)| SearchHit {
            summary: JobSummary::from(&job),
            score: Some(score),
        })
        .collect();
    tracing::info!(
        embed_ms,
        query_ms,
        filtered,
        limit,
        total,
        hits = results.len(),
        "search"
    );

    Ok(SearchResults { total, results })
}
