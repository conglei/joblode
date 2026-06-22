//! The cheap-model boundary. Ranking quality can be sharpened by a small LLM
//! pass over the *top* candidates only (never the whole set — that's the taste
//! ranker's job), so this trait is the seam we mock in tests and back with a
//! thin Gemini client in production.

use std::cmp::Ordering;

use async_trait::async_trait;

/// The compact text a model needs to judge a role. We never send full JDs to the
/// refinement pass — title + one-line summary is enough to compare fit.
#[derive(Debug, Clone)]
pub struct JobText {
    pub id: String,
    pub title: String,
    pub summary: String,
}

/// An absolute fit assessment for one role.
#[derive(Debug, Clone)]
pub struct MatchScore {
    /// Fit on a 0–100 scale.
    pub score: f32,
    /// One-line justification, surfaced to the user as `why`.
    pub why: String,
}

/// A cheap model used to refine the top of the candidate list.
#[async_trait]
pub trait ModelClient: Send + Sync {
    /// Absolute per-role fit (0–100) plus a one-line reason.
    async fn match_score(&self, resume: &str, job: &JobText) -> anyhow::Result<MatchScore>;

    /// Which role fits the resume better. `Ordering::Greater` means `a` is the
    /// better fit, `Less` means `b`, `Equal` a tie.
    async fn compare(&self, resume: &str, a: &JobText, b: &JobText) -> anyhow::Result<Ordering>;
}

/// A deterministic in-memory model for tests: fit is a caller-supplied score per
/// id, and comparisons follow those scores. No network, fully reproducible.
#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub struct FakeClient {
    scores: std::collections::HashMap<String, f32>,
}

#[cfg(test)]
impl FakeClient {
    pub fn new(scores: impl IntoIterator<Item = (&'static str, f32)>) -> Self {
        Self {
            scores: scores
                .into_iter()
                .map(|(id, s)| (id.to_string(), s))
                .collect(),
        }
    }

    fn score_of(&self, id: &str) -> f32 {
        self.scores.get(id).copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
#[async_trait]
impl ModelClient for FakeClient {
    async fn match_score(&self, _resume: &str, job: &JobText) -> anyhow::Result<MatchScore> {
        Ok(MatchScore {
            score: self.score_of(&job.id),
            why: format!("planted score {}", self.score_of(&job.id)),
        })
    }

    async fn compare(&self, _resume: &str, a: &JobText, b: &JobText) -> anyhow::Result<Ordering> {
        Ok(self
            .score_of(&a.id)
            .partial_cmp(&self.score_of(&b.id))
            .unwrap_or(Ordering::Equal))
    }
}
