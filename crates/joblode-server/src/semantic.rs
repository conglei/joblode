//! Shared semantic-search orchestration behind both the MCP `semantic_search`
//! tool and the REST `/api/semantic` endpoint. Embeds the free-text query into
//! the corpus's vector space, then ranks roles by best-variant cosine similarity
//! in DuckDB. Reuses [`crate::ranking::RankError`] so each face maps it the same
//! way (BadRequest → invalid_params/400, Internal → internal_error/500).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use joblode_core::JobStore;
use joblode_rank::EmbedClient;

use crate::dto::{JobSummary, SemanticHit, SemanticParams, SemanticResults, MAX_LIMIT};
use crate::ranking::RankError;

/// Default number of semantic hits to return.
const SEMANTIC_TOP: usize = 25;

/// Runs a semantic search: embed the query, then cosine-rank the (filtered) corpus.
///
/// # Errors
///
/// [`RankError::BadRequest`] for an empty query or when no embeddings model is
/// configured; [`RankError::Internal`] for an embedding-call or query failure.
pub async fn run(
    store: Arc<Mutex<JobStore>>,
    embed: Option<Arc<dyn EmbedClient>>,
    params: SemanticParams,
) -> Result<SemanticResults, RankError> {
    let query = params.query.trim().to_owned();
    if query.is_empty() {
        return Err(RankError::BadRequest(
            "semantic search requires a non-empty query".to_owned(),
        ));
    }
    let embed = embed.ok_or_else(|| {
        RankError::BadRequest(
            "semantic search requires a configured embeddings model; none is set".to_owned(),
        )
    })?;

    let embed_started = Instant::now();
    let vector = embed
        .embed(&query)
        .await
        .map_err(|error| RankError::Internal(format!("embedding failed: {error}")))?;
    let embed_ms = embed_started.elapsed().as_millis();

    let criteria = params.filter.criteria();
    let limit = params.filter.limit.unwrap_or(SEMANTIC_TOP).min(MAX_LIMIT);
    // A filtered query prunes the corpus before the cosine scan; an unfiltered one
    // scans every role — worth knowing when a query is slow (see DESIGN §6).
    let filtered = !criteria.is_empty();
    // `Some(d)` = the fast sidecar path (scan d-dim compact vectors); `None` = the
    // brute-force scan over the full 1536-d embedding columns.
    let index_dim = store
        .lock()
        .expect("store mutex poisoned")
        .semantic_index_dim();

    // Logged before the cosine scan so a long query is visible while it runs, not
    // only when it returns. Brute-force + `filtered=false` is the usual slow case.
    tracing::info!(
        filtered,
        sidecar = index_dim.is_some(),
        scan_dim = index_dim.unwrap_or(vector.len()),
        limit,
        "semantic_search: starting cosine scan"
    );

    let query_started = Instant::now();
    let hits = tokio::task::spawn_blocking(move || {
        let store = store.lock().expect("store mutex poisoned");
        store.semantic_search(&vector, &criteria, limit)
    })
    .await
    .map_err(|error| RankError::Internal(format!("semantic task failed: {error}")))?
    .map_err(|error| RankError::Internal(format!("semantic search failed: {error}")))?;
    let query_ms = query_started.elapsed().as_millis();

    tracing::info!(
        embed_ms,
        query_ms,
        filtered,
        sidecar = index_dim.is_some(),
        limit,
        hits = hits.len(),
        "semantic_search"
    );

    let results = hits
        .into_iter()
        .map(|(job, score)| SemanticHit {
            summary: JobSummary::from(&job),
            score,
        })
        .collect();

    Ok(SemanticResults { results })
}
