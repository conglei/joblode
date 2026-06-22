//! Shared ranking orchestration behind both the MCP `rank_jobs` tool and the REST
//! `/api/rank` endpoint — the "one core, many faces" rule (DESIGN §2/§6). It draws
//! a candidate set, learns a free taste order from feedback, and optionally
//! refines the top with a cheap model. Each face maps [`RankError`] to its own
//! error type.

use std::sync::{Arc, Mutex};

use joblode_core::{Criteria, Job, JobStore};
use joblode_rank::{Candidate, Method, ModelClient, RankRequest};

use crate::dto::{FeedbackItem, RankParams, RankResults};

/// Default ranked-shortlist size. Rank is the finalization step over the whole
/// matching set, so its shortlist is larger than a search page.
const RANK_TOP: usize = 25;

/// How many candidates to draw (by hard filter) before ranking, when ranking from
/// criteria rather than explicit ids. Rank orders the whole matching set, not a
/// page — the free taste ranker is cheap, so this is generous.
const RANK_CANDIDATE_LIMIT: usize = 1000;

/// Hard ceiling on the candidate draw, so the rank path can rank far more than a
/// search page (`MAX_LIMIT`) without being unbounded.
const RANK_MAX_CANDIDATES: usize = 2000;

/// How many taste-ordered candidates the `match` pass scores (one model call each).
const REFINE_MATCH: usize = 20;

/// How many the `pairwise` pass compares — smaller, since it is O(n²) calls.
const REFINE_PAIRWISE: usize = 8;

/// A ranking failure, mapped to each face's error type by the caller.
pub enum RankError {
    /// The request can't be served as asked — unknown method, missing resume, or
    /// a model-backed method with no model configured. (invalid-params / 400)
    BadRequest(String),
    /// A query or model failure. (internal-error / 500)
    Internal(String),
}

/// Candidates plus the feedback embeddings, gathered in one blocking DB pass.
struct Prepared {
    candidates: Vec<Candidate>,
    positives: Vec<Vec<f32>>,
    negatives: Vec<Vec<f32>>,
}

/// Runs the full rank funnel for `params`: draw candidates → free taste pre-rank
/// → optional cheap-model refinement → compact shortlist.
///
/// # Errors
///
/// [`RankError::BadRequest`] for an unknown method, a model method without a
/// configured model, or a missing resume; [`RankError::Internal`] for a query or
/// model-call failure.
pub async fn run(
    store: Arc<Mutex<JobStore>>,
    model: Option<Arc<dyn ModelClient>>,
    params: RankParams,
) -> Result<RankResults, RankError> {
    let method = parse_method(params.method.as_deref())?;

    // Validate model-method preconditions up front, so any later failure from the
    // rank call is unambiguously internal (no error-string sniffing).
    if matches!(method, Method::Match | Method::Pairwise) {
        let name = if method == Method::Match {
            "match"
        } else {
            "pairwise"
        };
        if model.is_none() {
            return Err(RankError::BadRequest(format!(
                "ranking method '{name}' requires a configured model; none is set"
            )));
        }
        let has_resume = params
            .resume
            .as_deref()
            .is_some_and(|resume| !resume.trim().is_empty());
        if !has_resume {
            return Err(RankError::BadRequest(format!(
                "ranking method '{name}' requires a resume"
            )));
        }
    }

    let top = params.top.unwrap_or(RANK_TOP);
    let refine_k = match method {
        Method::Pairwise => REFINE_PAIRWISE,
        _ => REFINE_MATCH,
    };

    let criteria = params.filter.criteria();
    // Clamp to the rank ceiling, so a client can't force an unbounded candidate
    // fetch — but rank can still order far more than a search page.
    let candidate_limit = params
        .filter
        .limit
        .unwrap_or(RANK_CANDIDATE_LIMIT)
        .min(RANK_MAX_CANDIDATES);
    let ids = params.ids;
    let feedback = params.feedback;
    let prep_store = store.clone();
    // The free taste path needs only embeddings; the model refine paths need the
    // candidates' title/summary for their prompts.
    let need_metadata = matches!(method, Method::Match | Method::Pairwise);

    // One blocking DB pass: draw candidates and the feedback embeddings.
    let prepared = tokio::task::spawn_blocking(move || {
        let store = prep_store.lock().expect("store mutex poisoned");
        prepare_candidates(
            &store,
            &criteria,
            candidate_limit,
            &ids,
            &feedback,
            need_metadata,
        )
    })
    .await
    .map_err(|error| RankError::Internal(format!("rank task failed: {error}")))?
    .map_err(|error| RankError::Internal(format!("rank prep failed: {error}")))?;

    let request = RankRequest {
        resume: params.resume.as_deref(),
        candidates: prepared.candidates,
        positives: prepared.positives,
        negatives: prepared.negatives,
        method,
        top,
        refine_k,
    };

    // Preconditions were checked above, so any failure here is internal.
    let results = joblode_rank::rank(model.as_deref(), request)
        .await
        .map_err(|error| RankError::Internal(error.to_string()))?;

    Ok(RankResults { results })
}

/// Parses the `method` string into a [`Method`], defaulting to free taste ranking.
fn parse_method(method: Option<&str>) -> Result<Method, RankError> {
    match method.map(|m| m.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("free") => Ok(Method::Free),
        Some("match") => Ok(Method::Match),
        Some("pairwise") => Ok(Method::Pairwise),
        Some(other) => Err(RankError::BadRequest(format!(
            "unknown rank method '{other}' (use 'match', 'pairwise', or omit)"
        ))),
    }
}

/// Draws the candidate set and resolves feedback ids to embeddings, all under one
/// held store lock. Candidates missing an embedding still rank (taste score 0).
///
/// `need_metadata` is the search↔rank boundary in code: the free taste path needs
/// only `(id, embedding)`, so it skips the wide row fetch entirely (id-only draw +
/// one embedding query); the model refine paths fetch full records for their
/// prompts.
fn prepare_candidates(
    store: &JobStore,
    criteria: &Criteria,
    candidate_limit: usize,
    ids: &[String],
    feedback: &[FeedbackItem],
    need_metadata: bool,
) -> anyhow::Result<Prepared> {
    // (id, title, summary) per candidate — title/summary empty on the free path.
    let candidates_meta: Vec<(String, String, String)> = if need_metadata {
        let jobs: Vec<Job> = if ids.is_empty() {
            store.search(criteria, candidate_limit)?.0
        } else {
            let mut found = Vec::with_capacity(ids.len());
            let mut seen = std::collections::HashSet::with_capacity(ids.len());
            for id in ids {
                if !seen.insert(id.as_str()) {
                    continue; // skip duplicate ids, keeping first-seen order
                }
                if let Some(job) = store.get_job(id)? {
                    found.push(job);
                }
            }
            found
        };
        jobs.into_iter()
            .map(|job| (job.id, job.title, job.role_summary))
            .collect()
    } else {
        // Free path: id-only draw — no per-id get_job, no wide row columns.
        let candidate_ids = if ids.is_empty() {
            store.candidate_ids(criteria, candidate_limit)?
        } else {
            let mut seen = std::collections::HashSet::with_capacity(ids.len());
            ids.iter()
                .filter(|id| seen.insert(id.as_str()))
                .cloned()
                .collect()
        };
        candidate_ids
            .into_iter()
            .map(|id| (id, String::new(), String::new()))
            .collect()
    };

    // Fetch embeddings for candidates and feedback ids together (deduplicated).
    let mut wanted: Vec<String> = candidates_meta
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect();
    wanted.extend(feedback.iter().map(|item| item.id.clone()));
    wanted.sort();
    wanted.dedup();
    let wanted_refs: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let embeddings = store.embeddings(&wanted_refs)?;

    let candidates = candidates_meta
        .into_iter()
        .map(|(id, title, summary)| {
            let embedding = embeddings.get(id.as_str()).cloned().unwrap_or_default();
            Candidate {
                id,
                title,
                summary,
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

/// Shared deterministic doubles for the MCP and REST ranking/semantic tests.
#[cfg(test)]
pub(crate) mod testing {
    use joblode_rank::{EmbedClient, JobText, MatchScore, ModelClient};

    /// Embeds any text to a fixed vector — lets semantic tests pick the target.
    pub(crate) struct FixedEmbed(pub Vec<f32>);

    #[async_trait::async_trait]
    impl EmbedClient for FixedEmbed {
        async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            Ok(self.0.clone())
        }
    }

    /// Scores the `favored` id at 90 and everything else at 10; `compare` orders
    /// by id. No network — fully reproducible.
    pub(crate) struct FavorId(pub &'static str);

    #[async_trait::async_trait]
    impl ModelClient for FavorId {
        async fn match_score(&self, _resume: &str, job: &JobText) -> anyhow::Result<MatchScore> {
            let score = if job.id == self.0 { 90.0 } else { 10.0 };
            Ok(MatchScore {
                score,
                why: format!("planted fit for {}", job.id),
            })
        }

        async fn compare(
            &self,
            _resume: &str,
            a: &JobText,
            b: &JobText,
        ) -> anyhow::Result<std::cmp::Ordering> {
            Ok(a.id.cmp(&b.id))
        }
    }
}
