//! Config-gated ranking for joblode.
//!
//! The funnel that keeps cloud token cost down: hard filters (in `joblode-core`)
//! cut ~1M roles to a candidate set, then [`rank`] orders that set and returns a
//! short, compact shortlist so the cloud model reads dozens of rows, not
//! thousands.
//!
//! Two layers, by cost:
//! - **Taste (free, keyless).** [`taste::TasteModel`] scores every candidate with
//!   vector math over embeddings already in the dataset, learned from the user's
//!   feedback. No model calls. This is the primary reducer.
//! - **Refinement (cheap model, optional).** [`Method::Match`] / [`Method::Pairwise`]
//!   sharpen only the top of the taste order via a [`ModelClient`]. Requires a
//!   configured key; absent, ranking degrades cleanly to the taste order.

mod embed;
mod gemini;
mod model;
mod pairwise;
mod taste;

pub use embed::{EmbedClient, OpenAiEmbedder};
pub use gemini::GeminiClient;
pub use model::{JobText, MatchScore, ModelClient};
pub use taste::TasteModel;

use std::cmp::Ordering;

use schemars::JsonSchema;
use serde::Serialize;

/// One ranked role, token-shaped: enough to triage, nothing more.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct Ranked {
    /// Dataset id; pass to `get_job` for the full record.
    pub id: String,
    /// Fit on a 0–100 scale (method-dependent, but always comparable within one
    /// result set).
    pub score: f32,
    /// One-line reason, when the method produces one (empty otherwise).
    pub why: String,
}

/// Which ranking method to apply on top of the free taste pre-rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Taste order only — no model calls.
    Free,
    /// Cheap-model absolute scoring of the top candidates.
    Match,
    /// Cheap-model pairwise comparison + Bradley–Terry over the top candidates.
    Pairwise,
}

/// A candidate role to rank: compact text for any model pass plus its embedding
/// for the taste pass.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub embedding: Vec<f32>,
}

/// A ranking request over an already-drawn candidate set.
pub struct RankRequest<'a> {
    /// Resume text, required by the model-backed methods.
    pub resume: Option<&'a str>,
    /// The candidate set (e.g. the output of a hard-filter search).
    pub candidates: Vec<Candidate>,
    /// Embeddings of roles the user liked (and any cold-start seed vector).
    pub positives: Vec<Vec<f32>>,
    /// Embeddings of roles the user disliked.
    pub negatives: Vec<Vec<f32>>,
    /// Which method to apply.
    pub method: Method,
    /// How many ranked rows to return.
    pub top: usize,
    /// How many of the taste-ordered top to hand to the model pass.
    pub refine_k: usize,
}

/// Maps a cosine similarity in `[-1, 1]` onto a `[0, 100]` score.
fn cosine_to_score(cosine: f32) -> f32 {
    ((cosine + 1.0) / 2.0) * 100.0
}

/// Ranks a candidate set into a compact shortlist.
///
/// The taste model (from `positives`/`negatives`) pre-orders the whole set for
/// free; the chosen [`Method`] then optionally refines the top `refine_k` with
/// `client`. Returns at most `top` rows.
///
/// # Errors
///
/// Returns an error if a model-backed method is requested but `client` is `None`
/// (ranking is unconfigured), if `resume` is missing for such a method, or if a
/// model call fails.
pub async fn rank(
    client: Option<&dyn ModelClient>,
    request: RankRequest<'_>,
) -> anyhow::Result<Vec<Ranked>> {
    let RankRequest {
        resume,
        candidates,
        positives,
        negatives,
        method,
        top,
        refine_k,
    } = request;

    // Free taste pre-rank: order the whole set, cheaply, by learned preference.
    let taste = TasteModel::from_examples(&positives, &negatives);
    let mut ordered: Vec<(Candidate, f32)> = candidates
        .into_iter()
        .map(|candidate| {
            let cosine = taste
                .as_ref()
                .map_or(0.0, |t| t.score(&candidate.embedding));
            (candidate, cosine)
        })
        .collect();
    // Stable-ish order: by taste desc; ties keep the incoming (search) order.
    ordered.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

    match method {
        Method::Free => Ok(ordered
            .into_iter()
            .take(top)
            .map(|(candidate, cosine)| Ranked {
                id: candidate.id,
                score: cosine_to_score(cosine),
                why: String::new(),
            })
            .collect()),
        Method::Match => refine_match(client, resume, ordered, top, refine_k).await,
        Method::Pairwise => refine_pairwise(client, resume, ordered, top, refine_k).await,
    }
}

/// Requires a configured client and resume for the model-backed methods.
fn require<'a>(
    client: Option<&'a dyn ModelClient>,
    resume: Option<&'a str>,
    method: &str,
) -> anyhow::Result<(&'a dyn ModelClient, &'a str)> {
    let client = client.ok_or_else(|| {
        anyhow::anyhow!("ranking method '{method}' requires a configured model; none is set")
    })?;
    let resume = resume
        .filter(|r| !r.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("ranking method '{method}' requires a resume"))?;
    Ok((client, resume))
}

/// The compact text head of the taste order handed to a model pass.
fn refine_set(ordered: &[(Candidate, f32)], refine_k: usize) -> Vec<JobText> {
    ordered
        .iter()
        .take(refine_k)
        .map(|(candidate, _)| JobText {
            id: candidate.id.clone(),
            title: candidate.title.clone(),
            summary: candidate.summary.clone(),
        })
        .collect()
}

async fn refine_match(
    client: Option<&dyn ModelClient>,
    resume: Option<&str>,
    ordered: Vec<(Candidate, f32)>,
    top: usize,
    refine_k: usize,
) -> anyhow::Result<Vec<Ranked>> {
    let (client, resume) = require(client, resume, "match")?;
    let head = refine_set(&ordered, refine_k);

    let mut scored = Vec::with_capacity(head.len());
    for job in &head {
        let assessment = client.match_score(resume, job).await?;
        scored.push(Ranked {
            id: job.id.clone(),
            score: assessment.score,
            why: assessment.why,
        });
    }
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
    scored.truncate(top);
    Ok(scored)
}

async fn refine_pairwise(
    client: Option<&dyn ModelClient>,
    resume: Option<&str>,
    ordered: Vec<(Candidate, f32)>,
    top: usize,
    refine_k: usize,
) -> anyhow::Result<Vec<Ranked>> {
    let (client, resume) = require(client, resume, "pairwise")?;
    let head = refine_set(&ordered, refine_k);

    let ranked = pairwise::rank(client, resume, &head).await?;
    let max = ranked
        .first()
        .map_or(1.0, |(_, s)| s.max(f32::MIN_POSITIVE));

    Ok(ranked
        .into_iter()
        .take(top)
        .map(|(index, strength)| Ranked {
            id: head[index].id.clone(),
            // Normalize Bradley–Terry strength to a 0–100 score within this set.
            score: (strength / max) * 100.0,
            why: String::new(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FakeClient;

    fn candidate(id: &str, embedding: [f32; 3]) -> Candidate {
        Candidate {
            id: id.to_string(),
            title: id.to_string(),
            summary: String::new(),
            embedding: embedding.to_vec(),
        }
    }

    fn candidates() -> Vec<Candidate> {
        vec![
            candidate("b", [0.0, 1.0, 0.0]),
            candidate("a", [1.0, 0.0, 0.0]),
            candidate("mid", [0.7, 0.7, 0.0]),
        ]
    }

    #[tokio::test]
    async fn free_method_orders_by_feedback_without_a_client() {
        // Liked an "a"-like role: the taste pass should float "a" to the top with
        // no model configured.
        let request = RankRequest {
            resume: None,
            candidates: candidates(),
            positives: vec![vec![1.0, 0.0, 0.0]],
            negatives: vec![],
            method: Method::Free,
            top: 3,
            refine_k: 3,
        };

        let ranked = rank(None, request).await.expect("free ranking");
        assert_eq!(ranked[0].id, "a");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[tokio::test]
    async fn free_method_keeps_search_order_without_feedback() {
        let request = RankRequest {
            resume: None,
            candidates: candidates(),
            positives: vec![],
            negatives: vec![],
            method: Method::Free,
            top: 2,
            refine_k: 2,
        };

        let ranked = rank(None, request).await.expect("free ranking");
        // No taste signal → incoming order preserved, capped at `top`.
        assert_eq!(
            ranked.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
    }

    #[tokio::test]
    async fn model_methods_error_cleanly_without_a_client() {
        for method in [Method::Match, Method::Pairwise] {
            let request = RankRequest {
                resume: Some("resume"),
                candidates: candidates(),
                positives: vec![],
                negatives: vec![],
                method,
                top: 2,
                refine_k: 2,
            };
            let error = rank(None, request)
                .await
                .expect_err("must require a client");
            assert!(error.to_string().contains("requires a configured model"));
        }
    }

    #[tokio::test]
    async fn match_refines_the_taste_order() {
        // Taste likes "a", but the model scores "mid" highest among the refine set.
        let client = FakeClient::new([("a", 10.0), ("b", 20.0), ("mid", 90.0)]);
        let request = RankRequest {
            resume: Some("resume"),
            candidates: candidates(),
            positives: vec![vec![1.0, 0.0, 0.0]],
            negatives: vec![],
            method: Method::Match,
            top: 3,
            refine_k: 3,
        };

        let ranked = rank(Some(&client), request).await.expect("match ranking");
        assert_eq!(ranked[0].id, "mid");
        assert_eq!(ranked[0].score, 90.0);
        assert!(ranked[0].why.contains("planted"));
    }

    #[tokio::test]
    async fn pairwise_refines_the_taste_order() {
        let client = FakeClient::new([("a", 1.0), ("b", 2.0), ("mid", 3.0)]);
        let request = RankRequest {
            resume: Some("resume"),
            candidates: candidates(),
            positives: vec![],
            negatives: vec![],
            method: Method::Pairwise,
            top: 3,
            refine_k: 3,
        };

        let ranked = rank(Some(&client), request)
            .await
            .expect("pairwise ranking");
        assert_eq!(
            ranked.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["mid", "b", "a"]
        );
        assert_eq!(ranked[0].score, 100.0); // top normalized to 100
    }
}
