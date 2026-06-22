//! Wire types shared by the MCP tools (`mcp`) and the REST API (`http`). Defining
//! them once keeps the two faces of `search_jobs`/`get_job` on one shape — the
//! "one core, many faces" rule in `docs/DESIGN.md` §2.

use joblode_core::{Criteria, Job};
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
    /// Max rows to return (default 50). Does not affect `total`.
    #[serde(default)]
    pub limit: Option<usize>,
}

impl SearchParams {
    /// The row cap to apply: the requested `limit` (or [`DEFAULT_LIMIT`]),
    /// clamped to [`MAX_LIMIT`] so a client can't force an unbounded page.
    #[must_use]
    pub fn effective_limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT)
    }

    /// Projects the filter fields onto [`Criteria`] (drops `limit`).
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

/// `search_jobs` result: the full match count plus a capped page of rows.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SearchResults {
    /// Total matching roles before `limit` is applied.
    pub total: usize,
    /// Compact rows, capped at `limit`. Call `get_job` for the full description.
    pub results: Vec<JobSummary>,
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
