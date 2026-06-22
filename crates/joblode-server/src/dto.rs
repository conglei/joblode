//! Wire types shared by the MCP tools (`mcp`) and the REST API (`http`). Defining
//! them once keeps the two faces of `search_jobs`/`get_job` on one shape — the
//! "one core, many faces" rule in `docs/DESIGN.md` §2.

use joblode_core::{Criteria, Job};
use joblode_rank::Ranked;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

/// Default cap on returned rows. `total` always reflects the full match count.
pub const DEFAULT_LIMIT: usize = 50;

/// Hard ceiling on returned rows, so a client can't request an unbounded page
/// (which would inflate query work and response size). `total` is unaffected.
pub const MAX_LIMIT: usize = 500;

/// Hard search filters plus a row cap, mirroring [`Criteria`] on the wire.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// Accepted job functions (exact match).
    #[serde(default)]
    pub functions: Vec<String>,
    /// Accepted seniority levels (exact match).
    #[serde(default)]
    pub levels: Vec<String>,
    /// Title terms; case-insensitive substrings, ORed together.
    #[serde(default)]
    pub titles: Vec<String>,
    /// Company terms matched across canonical and raw company names.
    #[serde(default)]
    pub companies: Vec<String>,
    /// City terms matched across city, region, and raw location.
    #[serde(default)]
    pub cities: Vec<String>,
    /// ISO alpha-2 country code; `US` also matches US-scoped remote roles.
    #[serde(default)]
    pub country: Option<String>,
    /// Minimum annual compensation in thousands (keeps unknown comp).
    #[serde(default)]
    pub min_comp: Option<f64>,
    /// Optional free-text description of the work. When set, results are ordered by
    /// semantic similarity of this query to each role (best of title / JD / alternate
    /// titles), under the same hard filters. Needs a configured embeddings model.
    #[serde(default)]
    pub query: Option<String>,
    /// Freshness window: only roles posted within the last N days (e.g. 14 for the
    /// past two weeks). Roles with an unknown post date are excluded when set.
    #[serde(default)]
    pub posted_within_days: Option<u32>,
    /// Max rows to return (default 50). Does not affect `total`.
    #[serde(default)]
    pub limit: Option<usize>,
}

impl SearchParams {
    /// The trimmed semantic query, if a non-empty one was given.
    #[must_use]
    pub fn semantic_query(&self) -> Option<&str> {
        self.query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
    }
}

impl SearchParams {
    /// The row cap to apply: the requested `limit` (or [`DEFAULT_LIMIT`]),
    /// clamped to [`MAX_LIMIT`] so a client can't force an unbounded page.
    #[must_use]
    pub fn effective_limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT)
    }

    /// Projects the filter fields onto [`Criteria`] (drops `limit`). Resolves the
    /// relative `posted_within_days` window to an absolute `posted_after` threshold.
    #[must_use]
    pub fn criteria(&self) -> Criteria {
        Criteria {
            functions: self.functions.clone(),
            levels: self.levels.clone(),
            titles: self.titles.clone(),
            companies: self.companies.clone(),
            cities: self.cities.clone(),
            country: self.country.clone(),
            min_comp: self.min_comp,
            posted_after: self.posted_within_days.map(|days| {
                (chrono::Utc::now() - chrono::Duration::days(i64::from(days))).to_rfc3339()
            }),
        }
    }
}

/// Token-shaped search row: enough to triage, without the full description.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct JobSummary {
    /// Dataset identifier; pass to `get_job` for the full record.
    pub id: String,
    /// Canonical company name.
    pub company: String,
    /// Posted job title.
    pub title: String,
    /// Raw location string.
    pub location: String,
    /// Extracted job function.
    pub function: String,
    /// Extracted seniority level.
    pub level: String,
    /// Extracted remote eligibility scope.
    pub remote_scope: String,
    /// Extracted minimum annual compensation in thousands (-1 if unknown).
    pub salary_min_k: f64,
    /// Extracted maximum annual compensation in thousands (-1 if unknown).
    pub salary_max_k: f64,
    /// One-line extracted role summary.
    pub role_summary: String,
    /// The only apply link — never fabricated.
    pub url: String,
}

impl From<&Job> for JobSummary {
    fn from(job: &Job) -> Self {
        Self {
            id: job.id.clone(),
            company: job.company.clone(),
            title: job.title.clone(),
            location: job.location.clone(),
            function: job.function.clone(),
            level: job.level.clone(),
            remote_scope: job.remote_scope.clone(),
            salary_min_k: job.salary_min_k,
            salary_max_k: job.salary_max_k,
            role_summary: job.role_summary.clone(),
            url: job.url.clone(),
        }
    }
}

/// One search row: a compact summary, plus a similarity `score` when the search
/// carried a semantic `query` (omitted for a plain keyword/filter search).
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SearchHit {
    #[serde(flatten)]
    pub summary: JobSummary,
    /// Best-variant cosine similarity to the query in `[-1, 1]`, when `query` was set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

/// `search` result: the match count plus a capped page of rows. With a semantic
/// `query`, rows are ordered by similarity and carry a `score`; otherwise it's a
/// plain filter page. Either way the hard filters define the set.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SearchResults {
    /// Total roles matching the hard filters — the candidate-set size, independent
    /// of `limit` and of any semantic ranking (the `query` ranks within this set).
    pub total: usize,
    /// Compact rows, capped at `limit`. Call `get_job` for the full description.
    pub results: Vec<SearchHit>,
}

/// One prior user reaction to a recommended role — the feedback-loop signal.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FeedbackItem {
    /// Dataset id of the role reacted to.
    pub id: String,
    /// `liked`/`applied`/`saved` (positive) or `disliked`/`rejected`/`skipped`
    /// (negative). Unrecognized labels are ignored.
    pub label: String,
}

impl FeedbackItem {
    /// `Some(true)` for a positive signal, `Some(false)` for negative, `None` if
    /// the label isn't recognized.
    #[must_use]
    pub fn polarity(&self) -> Option<bool> {
        match self.label.trim().to_ascii_lowercase().as_str() {
            "liked" | "like" | "applied" | "saved" | "shortlisted" => Some(true),
            "disliked" | "dislike" | "rejected" | "skipped" | "hidden" => Some(false),
            _ => None,
        }
    }
}

/// `rank_jobs` input: a candidate source (hard filters or explicit `ids`), the
/// resume + method for the optional model pass, and prior `feedback`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RankParams {
    /// Hard filters used to draw candidates (ignored when `ids` is given).
    #[serde(flatten)]
    pub filter: SearchParams,
    /// Explicit candidate ids to rank instead of running a filter search.
    #[serde(default)]
    pub ids: Vec<String>,
    /// Resume text; required by the `match` and `pairwise` methods.
    #[serde(default)]
    pub resume: Option<String>,
    /// Prior reactions, used to personalize the free taste ranking.
    #[serde(default)]
    pub feedback: Vec<FeedbackItem>,
    /// `match` or `pairwise` (needs a configured model). Omit for the free,
    /// keyless taste ranking.
    #[serde(default)]
    pub method: Option<String>,
    /// Max ranked rows to return (default 10).
    #[serde(default)]
    pub top: Option<usize>,
}

/// `rank_jobs` result: a compact, ordered shortlist.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RankResults {
    /// Ranked rows, best first. Call `get_job` for a role's full description.
    pub results: Vec<Ranked>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_limit_defaults_then_clamps() {
        let mut params = SearchParams::default();
        assert_eq!(params.effective_limit(), DEFAULT_LIMIT);

        params.limit = Some(10);
        assert_eq!(params.effective_limit(), 10);

        params.limit = Some(MAX_LIMIT * 100);
        assert_eq!(params.effective_limit(), MAX_LIMIT);
    }
}
