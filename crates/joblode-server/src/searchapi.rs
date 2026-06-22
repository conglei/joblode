//! A live external job source: SearchAPI's `google_jobs` engine, ported from the
//! TypeScript `JobSearchService`. It paginates Google Jobs results and normalizes
//! them into joblode's [`Job`] shape so they can be federated with the local corpus
//! and ranked in one space (see DESIGN — federated multi-source search).
//!
//! Cross-source identity is `sha256(apply_url)` (prefixed `googlejobs:`), so a role
//! that also appears in the corpus can be deduplicated by URL. Google Jobs lacks
//! joblode's LLM-extracted structured fields (function, comp, remote scope), so
//! those come back unknown; the description (`jd_markdown`) carries the detail.
//!
//! This is the ported source; federating it into `search::run` (fan-out, dedup,
//! on-demand embedding so external results join the taste/query ranking) is the
//! next slice — so the public API is allowed to be unused until then.
#![allow(dead_code)]

use std::time::Duration;

use anyhow::{Context, Result};
use joblode_core::Job;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const DEFAULT_BASE_URL: &str = "https://www.searchapi.io/api/v1/search";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// SearchAPI returns 10 results per page.
const RESULTS_PER_PAGE: usize = 10;

/// How recently a role was posted (folded into the query as Google understands it).
#[derive(Debug, Clone, Copy)]
pub enum PostTimeFilter {
    Yesterday,
    Past3Days,
    PastWeek,
}

/// Inputs for one Google Jobs search.
#[derive(Debug, Default)]
pub struct SearchApiParams {
    pub query: String,
    pub location: Option<String>,
    pub company: Option<String>,
    pub post_time_filter: Option<PostTimeFilter>,
    /// Desired number of results; drives how many 10-result pages we fetch.
    pub page_size: usize,
    /// Continuation token from a prior result (fetches exactly one more page).
    pub page_token: Option<String>,
}

/// A Google Jobs search client. Build it with [`SearchApiClient::new`]; one client
/// is reused across requests (it holds a pooled HTTP client).
pub struct SearchApiClient {
    api_key: String,
    base_url: String,
    /// Hard cap on pages fetched for one (non-token) search, bounding cost/latency.
    max_requests_per_search: usize,
    http: reqwest::Client,
}

impl SearchApiClient {
    /// Builds a client. Empty `base_url` falls back to the public SearchAPI endpoint.
    #[must_use]
    pub fn new(api_key: String, base_url: String, max_requests_per_search: usize) -> Self {
        let base_url = if base_url.trim().is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        Self {
            api_key,
            base_url,
            max_requests_per_search: max_requests_per_search.max(1),
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client builds with a timeout"),
        }
    }

    /// Searches Google Jobs, paginating until `page_size` results are gathered (or
    /// the cap is hit, or results run out). A `page_token` fetches exactly one page.
    /// Returns `(jobs, next_page_token)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the first page request fails. Once some results are in
    /// hand, a later page error stops pagination and returns the partial set.
    pub async fn search_jobs(
        &self,
        params: &SearchApiParams,
    ) -> Result<(Vec<Job>, Option<String>)> {
        // A continuation request is a single page.
        if params.page_token.is_some() {
            return self.fetch_page(params, params.page_token.as_deref()).await;
        }

        let want = params.page_size.max(RESULTS_PER_PAGE);
        let pages_needed = want.div_ceil(RESULTS_PER_PAGE);
        let max_pages = pages_needed.min(self.max_requests_per_search);

        let mut jobs: Vec<Job> = Vec::new();
        let mut next_token: Option<String> = None;
        for page in 0..max_pages {
            match self.fetch_page(params, next_token.as_deref()).await {
                Ok((mut page_jobs, token)) => {
                    let count = page_jobs.len();
                    jobs.append(&mut page_jobs);
                    next_token = token;
                    // Stop when there's nothing more, or a short page signals the end.
                    if next_token.is_none() || count < RESULTS_PER_PAGE {
                        break;
                    }
                    if jobs.len() >= want {
                        break;
                    }
                }
                Err(error) => {
                    // First page failed → surface it; otherwise keep partial results.
                    if jobs.is_empty() {
                        return Err(error.context(format!("google_jobs page {}", page + 1)));
                    }
                    tracing::warn!(page = page + 1, %error, "google_jobs page failed; partial results");
                    break;
                }
            }
        }
        Ok((jobs, next_token))
    }

    /// Fetches and transforms one page.
    async fn fetch_page(
        &self,
        params: &SearchApiParams,
        page_token: Option<&str>,
    ) -> Result<(Vec<Job>, Option<String>)> {
        let query = build_query(params);
        let mut request = self.http.get(&self.base_url).query(&[
            ("engine", "google_jobs"),
            ("q", query.as_str()),
            ("api_key", self.api_key.as_str()),
            ("google_domain", "google.com"),
            ("gl", "us"),
            ("hl", "en"),
        ]);
        if let Some(token) = page_token {
            request = request.query(&[("next_page_token", token)]);
        }

        let response = request
            .send()
            .await
            .context("searchapi request failed")?
            .error_for_status()
            .context("searchapi returned an error status")?
            .json::<SearchApiResponse>()
            .await
            .context("searchapi response was not the expected json")?;

        let jobs = response.jobs.into_iter().map(transform).collect();
        let next = response.pagination.and_then(|p| p.next_page_token);
        Ok((jobs, next))
    }
}

/// Builds the free-text query Google Jobs expects: the base query, then location,
/// company, and a recency phrase (defaulting to the past week).
fn build_query(params: &SearchApiParams) -> String {
    let mut query = params.query.clone();
    if let Some(location) = params.location.as_deref().filter(|s| !s.is_empty()) {
        query.push_str(&format!(" in {location}"));
    }
    if let Some(company) = params.company.as_deref().filter(|s| !s.is_empty()) {
        query.push_str(&format!(" at {company}"));
    }
    query.push_str(match params.post_time_filter {
        Some(PostTimeFilter::Yesterday) => " since yesterday",
        Some(PostTimeFilter::Past3Days) => " since the past 3 days",
        Some(PostTimeFilter::PastWeek) => " since the past week",
        None => " posted in the past week",
    });
    query
}

// — SearchAPI response shapes (only the fields we use) ————————————————————————

#[derive(Debug, Deserialize)]
struct SearchApiResponse {
    #[serde(default)]
    jobs: Vec<SearchApiJob>,
    pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize)]
struct Pagination {
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchApiJob {
    #[serde(default)]
    title: String,
    #[serde(default)]
    company_name: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    job_highlights: Vec<JobHighlight>,
    detected_extensions: Option<DetectedExtensions>,
    apply_link: Option<String>,
    #[serde(default)]
    apply_links: Vec<ApplyLink>,
}

#[derive(Debug, Deserialize)]
struct JobHighlight {
    #[serde(default)]
    title: String,
    #[serde(default)]
    items: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApplyLink {
    #[serde(default)]
    link: String,
}

#[derive(Debug, Default, Deserialize)]
struct DetectedExtensions {
    schedule: Option<String>,
    salary: Option<String>,
    posted_at: Option<String>,
    #[serde(default)]
    health_insurance: bool,
    #[serde(default)]
    dental_insurance: bool,
    #[serde(default)]
    paid_time_off: bool,
}

// — Transform: SearchAPI job → joblode Job ————————————————————————————————————

/// Normalizes one SearchAPI job into joblode's [`Job`]. The rich, source-specific
/// detail (highlights, benefits, salary, posted date) is rendered into `jd_markdown`
/// since [`Job`] has no slots for it; the structured fields Google Jobs doesn't
/// provide come back empty.
fn transform(job: SearchApiJob) -> Job {
    let ext = job.detected_extensions.unwrap_or_default();

    let requirements = highlight_items(&job.job_highlights, &["qualification", "requirement"]);
    let responsibilities = highlight_items(&job.job_highlights, &["responsibilit"]);
    let mut benefits = highlight_items(&job.job_highlights, &["benefit"]);
    if ext.health_insurance {
        benefits.push("Health Insurance".to_string());
    }
    if ext.dental_insurance {
        benefits.push("Dental Insurance".to_string());
    }
    if ext.paid_time_off {
        benefits.push("Paid Time Off".to_string());
    }

    // Apply links: primary first, then the rest, all UTM-cleaned and deduplicated.
    let mut apply_links: Vec<String> = Vec::new();
    let primary = job
        .apply_link
        .as_deref()
        .or_else(|| job.apply_links.first().map(|l| l.link.as_str()))
        .map(clean_url)
        .unwrap_or_default();
    if !primary.is_empty() {
        apply_links.push(primary.clone());
    }
    for link in &job.apply_links {
        let cleaned = clean_url(&link.link);
        if !cleaned.is_empty() && !apply_links.contains(&cleaned) {
            apply_links.push(cleaned);
        }
    }

    let level = experience_level(&job.title, &job.description);
    let posted = ext
        .posted_at
        .or_else(|| {
            job.job_highlights
                .iter()
                .flat_map(|h| &h.items)
                .find(|item| is_relative_date(item))
                .cloned()
        })
        .map(|s| parse_relative_date(&s));

    let jd_markdown = render_markdown(
        &job.description,
        &requirements,
        &responsibilities,
        &benefits,
        ext.salary.as_deref(),
        ext.schedule.as_deref(),
        posted.as_deref(),
        &apply_links,
    );

    Job {
        id: format!("googlejobs:{}", job_id(&primary)),
        company: job.company_name,
        title: job.title,
        url: primary,
        function: String::new(),
        sub_function: String::new(),
        level,
        work_mode: ext.schedule.clone().unwrap_or_default(),
        remote_scope: String::new(),
        country_code: String::new(),
        // Google Jobs gives a fuzzy salary string, not a normalized range; leave the
        // numeric fields unknown and keep the string in jd_markdown.
        salary_min_k: -1.0,
        salary_max_k: -1.0,
        location: job.location.clone(),
        city: String::new(),
        region: String::new(),
        role_summary: summarize(&job.description),
        jd_markdown,
    }
}

/// Items from the first highlight whose title contains any of `keywords`.
fn highlight_items(highlights: &[JobHighlight], keywords: &[&str]) -> Vec<String> {
    highlights
        .iter()
        .find(|h| {
            let title = h.title.to_lowercase();
            keywords.iter().any(|k| title.contains(k))
        })
        .map(|h| h.items.clone())
        .unwrap_or_default()
}

/// Coarse seniority from title + description keywords (mirrors the TS heuristic).
fn experience_level(title: &str, description: &str) -> String {
    let text = format!("{title} {description}").to_lowercase();
    if ["senior", "lead", "director", "manager"]
        .iter()
        .any(|k| text.contains(k))
    {
        "Senior".to_string()
    } else if ["junior", "entry", "intern"]
        .iter()
        .any(|k| text.contains(k))
    {
        "Entry".to_string()
    } else if ["mid", "intermediate"].iter().any(|k| text.contains(k)) {
        "Mid".to_string()
    } else {
        String::new()
    }
}

/// A one-line summary: the first sentence/line of the description, capped.
fn summarize(description: &str) -> String {
    let first = description
        .split(['\n', '.'])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("");
    if first.chars().count() > 160 {
        format!("{}…", first.chars().take(159).collect::<String>())
    } else {
        first.to_string()
    }
}

/// Renders the source-specific detail into a single markdown document.
#[allow(clippy::too_many_arguments)]
fn render_markdown(
    description: &str,
    requirements: &[String],
    responsibilities: &[String],
    benefits: &[String],
    salary: Option<&str>,
    schedule: Option<&str>,
    posted: Option<&str>,
    apply_links: &[String],
) -> String {
    let mut out = String::new();
    let meta: Vec<String> = [
        salary.map(|s| format!("**Salary:** {s}")),
        schedule.map(|s| format!("**Schedule:** {s}")),
        posted.map(|s| format!("**Posted:** {s}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !meta.is_empty() {
        out.push_str(&meta.join(" · "));
        out.push_str("\n\n");
    }
    out.push_str(description.trim());

    let mut section = |heading: &str, items: &[String]| {
        if !items.is_empty() {
            out.push_str(&format!("\n\n## {heading}\n"));
            for item in items {
                out.push_str(&format!("- {item}\n"));
            }
        }
    };
    section("Responsibilities", responsibilities);
    section("Requirements", requirements);
    section("Benefits", benefits);

    if apply_links.len() > 1 {
        out.push_str("\n\n## Apply\n");
        for link in apply_links {
            out.push_str(&format!("- {link}\n"));
        }
    }
    out
}

/// A stable 16-hex-char id from the apply URL (sha256), for cross-source dedup.
fn job_id(apply_url: &str) -> String {
    let digest = Sha256::digest(apply_url.as_bytes());
    let hex = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    hex[..16].to_string()
}

/// Strips UTM parameters from a URL; returns the input unchanged if it won't parse.
fn clean_url(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    match url::Url::parse(raw) {
        Ok(mut parsed) => {
            let kept: Vec<(String, String)> = parsed
                .query_pairs()
                .filter(|(k, _)| {
                    !matches!(k.as_ref(), "utm_campaign" | "utm_source" | "utm_medium")
                })
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            parsed.set_query(None);
            if !kept.is_empty() {
                parsed.query_pairs_mut().extend_pairs(kept);
            }
            parsed.to_string()
        }
        Err(_) => raw.to_string(),
    }
}

/// True if a string looks like a Google Jobs relative date ("3 days ago", …).
fn is_relative_date(s: &str) -> bool {
    let s = s.to_lowercase();
    s.contains("ago")
        || s.contains("day")
        || s.contains("week")
        || s.contains("month")
        || s.contains("hour")
}

/// Converts a relative date ("2 hours ago", "3 days ago", "yesterday", "today") to
/// an RFC3339 timestamp; returns the input unchanged if it can't be parsed.
fn parse_relative_date(raw: &str) -> String {
    use chrono::{Duration, Utc};
    let now = Utc::now();
    let lower = raw.to_lowercase();
    let lower = lower.trim();

    let amount = |unit: &str| -> Option<i64> {
        lower
            .split_whitespace()
            .position(|w| w.starts_with(unit))
            .and_then(|i| lower.split_whitespace().nth(i.wrapping_sub(1)))
            .and_then(|n| n.parse::<i64>().ok())
    };

    let delta = if let Some(h) = amount("hour") {
        Some(Duration::hours(h))
    } else if let Some(d) = amount("day") {
        Some(Duration::days(d))
    } else if let Some(w) = amount("week") {
        Some(Duration::weeks(w))
    } else if let Some(m) = amount("month") {
        Some(Duration::days(m * 30))
    } else if lower.contains("yesterday") {
        Some(Duration::days(1))
    } else if lower.contains("today") {
        Some(Duration::zero())
    } else {
        None
    };

    match delta {
        Some(delta) => (now - delta).to_rfc3339(),
        None => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_query_appends_location_company_and_recency() {
        let params = SearchApiParams {
            query: "backend engineer".into(),
            location: Some("San Francisco".into()),
            company: Some("Acme".into()),
            post_time_filter: Some(PostTimeFilter::Past3Days),
            ..Default::default()
        };
        assert_eq!(
            build_query(&params),
            "backend engineer in San Francisco at Acme since the past 3 days"
        );
    }

    #[test]
    fn build_query_defaults_recency_to_past_week() {
        let params = SearchApiParams {
            query: "data engineer".into(),
            ..Default::default()
        };
        assert_eq!(
            build_query(&params),
            "data engineer posted in the past week"
        );
    }

    #[test]
    fn clean_url_strips_utm_params_only() {
        assert_eq!(
            clean_url("https://x.com/job?utm_source=google&id=42&utm_medium=cpc"),
            "https://x.com/job?id=42"
        );
        assert_eq!(clean_url("https://x.com/job"), "https://x.com/job");
        assert_eq!(clean_url("not a url"), "not a url");
    }

    #[test]
    fn job_id_is_stable_and_short() {
        let a = job_id("https://x.com/apply");
        assert_eq!(a.len(), 16);
        assert_eq!(a, job_id("https://x.com/apply"));
        assert_ne!(a, job_id("https://x.com/other"));
    }

    #[test]
    fn experience_level_from_keywords() {
        assert_eq!(experience_level("Senior Backend Engineer", ""), "Senior");
        assert_eq!(experience_level("Software Intern", ""), "Entry");
        assert_eq!(experience_level("Mid-level Designer", ""), "Mid");
        assert_eq!(experience_level("Software Engineer", "build things"), "");
    }

    #[test]
    fn transform_maps_a_searchapi_job_to_a_normalized_job() {
        let raw = serde_json::json!({
            "title": "Senior Backend Engineer",
            "company_name": "Acme",
            "location": "San Francisco, CA",
            "description": "Own the API. Build resilient services.",
            "job_highlights": [
                { "title": "Qualifications", "items": ["5+ years Rust"] },
                { "title": "Responsibilities", "items": ["Design systems"] }
            ],
            "detected_extensions": { "salary": "$180K–$220K a year", "schedule": "Full-time" },
            "apply_link": "https://acme.com/apply?utm_source=google"
        });
        let job: SearchApiJob = serde_json::from_value(raw).expect("parse fixture");
        let job = transform(job);

        assert!(job.id.starts_with("googlejobs:"));
        assert_eq!(job.company, "Acme");
        assert_eq!(job.title, "Senior Backend Engineer");
        assert_eq!(job.url, "https://acme.com/apply"); // utm stripped
        assert_eq!(job.level, "Senior");
        assert_eq!(job.salary_min_k, -1.0); // fuzzy salary stays unknown
        assert!(job.jd_markdown.contains("$180K–$220K")); // but is in the JD
        assert!(job.jd_markdown.contains("5+ years Rust"));
        assert_eq!(job.role_summary, "Own the API");
    }
}
