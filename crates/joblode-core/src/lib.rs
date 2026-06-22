//! DuckDB-backed search and retrieval over the open-jobs parquet dataset.

use std::collections::HashMap;
use std::path::Path;

use duckdb::{params_from_iter, types::Value, Connection, Error, OptionalExt, Result, Row};

/// Returns the crate version.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Hard eligibility filters for a job search.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Criteria {
    /// Accepted job functions.
    pub functions: Vec<String>,
    /// Accepted seniority levels.
    pub levels: Vec<String>,
    /// Title terms matched as case-insensitive substrings.
    pub titles: Vec<String>,
    /// Company terms matched across canonical and raw company names.
    pub companies: Vec<String>,
    /// City terms matched across city, region, and raw location.
    pub cities: Vec<String>,
    /// Required ISO alpha-2 country code.
    pub country: Option<String>,
    /// Annual compensation floor in thousands.
    pub min_comp: Option<f64>,
    /// Only roles posted on or after this RFC3339 timestamp (a freshness window).
    /// Roles with a missing or unparseable `posted_at` are excluded when set.
    pub posted_after: Option<String>,
}

impl Criteria {
    /// True when no hard filter is set — every role passes. A semantic search over
    /// empty criteria scans the whole corpus's embeddings (see DESIGN §6).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
            && self.levels.is_empty()
            && self.titles.is_empty()
            && self.companies.is_empty()
            && self.cities.is_empty()
            && self.country.is_none()
            && self.min_comp.is_none()
            && self.posted_after.is_none()
    }
}

/// A job record returned by search or retrieval.
#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct Job {
    /// Dataset identifier.
    pub id: String,
    /// Canonical company name when available.
    pub company: String,
    /// Posted job title.
    pub title: String,
    /// Application URL.
    pub url: String,
    /// Extracted job function.
    pub function: String,
    /// Extracted job sub-function.
    pub sub_function: String,
    /// Extracted seniority level.
    pub level: String,
    /// Extracted work mode.
    pub work_mode: String,
    /// Extracted remote eligibility scope.
    pub remote_scope: String,
    /// Extracted ISO alpha-2 country code.
    pub country_code: String,
    /// Extracted minimum annual compensation in thousands.
    pub salary_min_k: f64,
    /// Extracted maximum annual compensation in thousands.
    pub salary_max_k: f64,
    /// Raw location string.
    pub location: String,
    /// Extracted city.
    pub city: String,
    /// Extracted region.
    pub region: String,
    /// One-line extracted role summary.
    pub role_summary: String,
    /// Full job description as Markdown.
    pub jd_markdown: String,
}

/// A compact embedding sidecar: `id` + a truncated `jd_embedding` (`FLOAT[dim]`),
/// built once from the dataset. Semantic search scans this instead of the full
/// embedding columns — far less I/O and `dim`× less compute (see DESIGN §6).
#[derive(Debug, Clone)]
struct Sidecar {
    path: String,
    dim: usize,
}

/// Read-only access to one parquet dataset, optionally with an embedding sidecar
/// for fast semantic search.
pub struct JobStore {
    connection: Connection,
    parquet: String,
    sidecar: Option<Sidecar>,
}

impl std::fmt::Debug for JobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The DuckDB connection is not `Debug`; expose only the dataset path.
        f.debug_struct("JobStore")
            .field("parquet", &self.parquet)
            .finish_non_exhaustive()
    }
}

impl JobStore {
    /// Opens and validates a local parquet dataset.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is not valid UTF-8, or if the parquet cannot
    /// be opened and read (missing file, unreadable, or not a parquet).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let parquet = path
            .to_str()
            .ok_or_else(|| Error::InvalidPath(path.to_path_buf()))?
            .to_owned();
        let connection = Connection::open_in_memory()?;
        connection.query_row("SELECT count(*) FROM read_parquet(?)", [&parquet], |_| {
            Ok(())
        })?;

        Ok(Self {
            connection,
            parquet,
            sidecar: None,
        })
    }

    /// Builds a compact embedding sidecar at `out_path`: each role's `id` and its
    /// `jd_embedding` truncated to the first `target_dim` dimensions, stored as a
    /// `FLOAT[dim]` parquet column. Truncation is the Matryoshka property of the
    /// `text-embedding-3-*` models — the prefix is still a usable embedding — and
    /// `array_cosine_similarity` renormalizes, so no separate normalize step is
    /// needed. Returns the effective `dim` written (`min(target_dim, actual)`).
    ///
    /// Run this once after a data refresh; point [`attach_sidecar`](Self::attach_sidecar)
    /// at the output to enable the fast path.
    ///
    /// # Errors
    ///
    /// Returns an error if the dataset has no usable `jd_embedding`, or the copy
    /// query fails (e.g. the output path is not writable).
    pub fn build_embedding_sidecar(
        &self,
        out_path: impl AsRef<Path>,
        target_dim: usize,
    ) -> Result<usize> {
        let out = out_path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::InvalidPath(out_path.as_ref().to_path_buf()))?;
        // The stored dimension can't exceed what the dataset actually carries.
        let actual: i64 = self.connection.query_row(
            "SELECT len(jd_embedding) FROM read_parquet(?) WHERE jd_embedding IS NOT NULL LIMIT 1",
            [&self.parquet],
            |row| row.get(0),
        )?;
        let dim = (usize::try_from(actual).unwrap_or(0))
            .min(target_dim)
            .max(1);
        let sql = format!(
            "COPY (SELECT cast(id AS VARCHAR) AS id, \
                    jd_embedding[1:{dim}]::FLOAT[{dim}] AS vec \
             FROM read_parquet('{main}') WHERE jd_embedding IS NOT NULL) \
             TO '{out}' (FORMAT PARQUET)",
            main = sql_quote(&self.parquet),
            out = sql_quote(out),
        );
        self.connection.execute(&sql, [])?;
        Ok(dim)
    }

    /// The attached sidecar's dimension, or `None` when semantic search runs the
    /// brute-force full-embedding scan. Lets callers log which path a query took.
    #[must_use]
    pub fn semantic_index_dim(&self) -> Option<usize> {
        self.sidecar.as_ref().map(|sidecar| sidecar.dim)
    }

    /// Enables the fast semantic-search path by attaching the sidecar at `path`
    /// (built by [`build_embedding_sidecar`](Self::build_embedding_sidecar)). Its
    /// dimension is read from the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is not valid UTF-8 or the sidecar can't be read.
    pub fn attach_sidecar(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::InvalidPath(path.as_ref().to_path_buf()))?
            .to_owned();
        let dim: i64 = self.connection.query_row(
            "SELECT len(vec) FROM read_parquet(?) LIMIT 1",
            [&path],
            |row| row.get(0),
        )?;
        // Reject a bogus dimension cleanly here rather than letting a `0`-d sidecar
        // surface as a confusing failure on the first semantic query.
        let dim =
            usize::try_from(dim).map_err(|_| Error::IntegralValueOutOfRange(0, i128::from(dim)))?;
        self.sidecar = Some(Sidecar { path, dim });
        Ok(())
    }

    /// Searches jobs and returns up to `limit` deduplicated rows plus the total
    /// match count. `total` reflects all matches; only the returned rows are
    /// capped, with `LIMIT` applied at the query level so unreturned rows are
    /// never materialized.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying SQL query fails (e.g. the dataset
    /// schema is missing an expected column).
    pub fn search(&self, criteria: &Criteria, limit: usize) -> Result<(Vec<Job>, usize)> {
        let mut parameters = vec![Value::Text(self.parquet.clone())];
        let filters = collect_filters(criteria, &mut parameters);

        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        };
        let sql = format!(
            r#"
            WITH filtered AS (
                SELECT
                    *,
                    coalesce(nullif(company_name, ''), company, '') AS display_company,
                    row_number() OVER (
                        PARTITION BY
                            lower(coalesce(nullif(company_name, ''), company, '')),
                            lower(coalesce(title, ''))
                        ORDER BY cast(id AS VARCHAR)
                    ) AS duplicate_rank
                FROM read_parquet(?)
                {where_clause}
            ),
            deduplicated AS (
                SELECT * FROM filtered WHERE duplicate_rank = 1
            )
            SELECT
                cast(id AS VARCHAR),
                display_company,
                coalesce(title, ''),
                coalesce(url, ''),
                coalesce("function", ''),
                coalesce(sub_function, ''),
                coalesce(level, ''),
                coalesce(work_mode, ''),
                coalesce(remote_scope, ''),
                coalesce(country_code, ''),
                coalesce(salary_min_k, -1),
                coalesce(salary_max_k, -1),
                coalesce(location, ''),
                coalesce(city, ''),
                coalesce(region, ''),
                coalesce(role_summary, ''),
                coalesce(jd_markdown, ''),
                count(*) OVER ()
            FROM deduplicated
            ORDER BY cast(id AS VARCHAR)
            LIMIT {limit}
            "#
        );

        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(parameters), |row| {
            Ok((job_from_row(row)?, row.get::<_, i64>(17)?))
        })?;

        let mut jobs = Vec::new();
        let mut total = 0;
        for row in rows {
            let (job, count) = row?;
            jobs.push(job);
            total = count as usize;
        }

        Ok((jobs, total))
    }

    /// Retrieves one full job by dataset identifier.
    ///
    /// Returns `Ok(None)` when no job has the given `id`, distinguishing a
    /// genuine miss from a query failure.
    ///
    /// # Errors
    ///
    /// Returns an error if the query itself fails.
    pub fn get_job(&self, id: &str) -> Result<Option<Job>> {
        self.connection
            .query_row(
                r#"
            SELECT
                cast(id AS VARCHAR),
                coalesce(nullif(company_name, ''), company, ''),
                coalesce(title, ''),
                coalesce(url, ''),
                coalesce("function", ''),
                coalesce(sub_function, ''),
                coalesce(level, ''),
                coalesce(work_mode, ''),
                coalesce(remote_scope, ''),
                coalesce(country_code, ''),
                coalesce(salary_min_k, -1),
                coalesce(salary_max_k, -1),
                coalesce(location, ''),
                coalesce(city, ''),
                coalesce(region, ''),
                coalesce(role_summary, ''),
                coalesce(jd_markdown, '')
            FROM read_parquet(?)
            WHERE cast(id AS VARCHAR) = ?
            LIMIT 1
            "#,
                [&self.parquet, id],
                job_from_row,
            )
            .optional()
    }

    /// Fetches the `jd_embedding` vector for each of `ids` that exists in the
    /// dataset. Ids with no row are simply omitted from the returned map.
    ///
    /// Embeddings are read as a delimited string (`array_to_string`) and parsed,
    /// so this does not depend on the driver's array-column support.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or an embedding value can't be parsed
    /// as a float.
    pub fn embeddings(&self, ids: &[&str]) -> Result<HashMap<String, Vec<f32>>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        // Read the compact sidecar vectors when attached (256-d, far less I/O),
        // else the full `jd_embedding`. Ranking uses these for both candidates and
        // feedback, so they stay in one space.
        let (source, column) = match &self.sidecar {
            Some(sidecar) => (sidecar.path.clone(), "vec"),
            None => (self.parquet.clone(), "jd_embedding"),
        };

        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        // coalesce: rows with a NULL embedding come back as "" (→ empty vec),
        // never as a NULL that would fail the string conversion.
        let sql = format!(
            "SELECT cast(id AS VARCHAR), coalesce(array_to_string({column}, ','), '') \
             FROM read_parquet(?) \
             WHERE cast(id AS VARCHAR) IN ({placeholders})"
        );

        let mut parameters: Vec<Value> = Vec::with_capacity(ids.len() + 1);
        parameters.push(Value::Text(source));
        parameters.extend(ids.iter().map(|id| Value::Text((*id).to_owned())));

        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(parameters), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut out = HashMap::with_capacity(ids.len());
        for row in rows {
            let (id, packed) = row?;
            let vector = parse_embedding(&packed)
                .map_err(|error| Error::ToSqlConversionFailure(Box::new(error)))?;
            out.insert(id, vector);
        }
        Ok(out)
    }

    /// Counts the deduplicated roles matching `criteria` — the size of the candidate
    /// set the hard filters define, independent of any ranking or row cap. Used to
    /// report `total` for a semantic search (the filters constrain; the query only
    /// ranks within them).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying query fails.
    pub fn count(&self, criteria: &Criteria) -> Result<usize> {
        let mut parameters = vec![Value::Text(self.parquet.clone())];
        let filters = collect_filters(criteria, &mut parameters);
        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        };
        let sql = format!(
            r#"
            WITH filtered AS (
                SELECT row_number() OVER (
                    PARTITION BY
                        lower(coalesce(nullif(company_name, ''), company, '')),
                        lower(coalesce(title, ''))
                    ORDER BY cast(id AS VARCHAR)
                ) AS duplicate_rank
                FROM read_parquet(?)
                {where_clause}
            )
            SELECT count(*) FROM filtered WHERE duplicate_rank = 1
            "#
        );
        let total: i64 = self
            .connection
            .query_row(&sql, params_from_iter(parameters), |row| row.get(0))?;
        Ok(usize::try_from(total).unwrap_or(0))
    }

    /// Returns up to `limit` deduplicated candidate ids matching `criteria`, ordered
    /// by id. A lightweight id-only projection for the fast (feedback-only) rank
    /// path — it draws the whole matching set to rank without reading the wide row
    /// columns that [`search`](Self::search) returns.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying query fails.
    pub fn candidate_ids(&self, criteria: &Criteria, limit: usize) -> Result<Vec<String>> {
        let mut parameters = vec![Value::Text(self.parquet.clone())];
        let filters = collect_filters(criteria, &mut parameters);
        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        };
        let sql = format!(
            r#"
            WITH filtered AS (
                SELECT
                    cast(id AS VARCHAR) AS id,
                    row_number() OVER (
                        PARTITION BY
                            lower(coalesce(nullif(company_name, ''), company, '')),
                            lower(coalesce(title, ''))
                        ORDER BY cast(id AS VARCHAR)
                    ) AS duplicate_rank
                FROM read_parquet(?)
                {where_clause}
            )
            SELECT id FROM filtered WHERE duplicate_rank = 1
            ORDER BY id
            LIMIT {limit}
            "#
        );

        let mut statement = self.connection.prepare(&sql)?;
        let rows =
            statement.query_map(params_from_iter(parameters), |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Semantic search: orders roles by cosine similarity of `query` to their
    /// embeddings, applying the same hard `criteria` filters as [`search`].
    ///
    /// Each role scores as the **best-matching variant** — the max cosine over
    /// its title, JD, and each alternate-title embedding — so a query matches the
    /// closest facet rather than a blurred average. Returns up to `limit` rows as
    /// `(job, similarity)`, best first. `query` must match the dataset's embedding
    /// dimension (1536 for text-embedding-3-small).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails (e.g. a dimension mismatch against the
    /// stored vectors).
    pub fn semantic_search(
        &self,
        query: &[f32],
        criteria: &Criteria,
        limit: usize,
    ) -> Result<Vec<(Job, f32)>> {
        if query.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(sidecar) = &self.sidecar {
            return self.semantic_search_sidecar(sidecar, query, criteria, limit);
        }
        let dim = query.len();
        // The query vector is our own computed data (no injection); inline it once
        // as a typed literal via a CTE so the cast/compare reuse it.
        let literal = query
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");

        let mut parameters = vec![Value::Text(self.parquet.clone())];
        let mut filters = collect_filters(criteria, &mut parameters);
        // A role needs at least one embedding to be matchable.
        filters.push(
            "(title_embedding IS NOT NULL OR jd_embedding IS NOT NULL \
             OR alt_titles_embedding IS NOT NULL)"
                .to_owned(),
        );
        let where_clause = format!("WHERE {}", filters.join(" AND "));

        let sql = format!(
            r#"
            WITH q AS (SELECT [{literal}]::FLOAT[{dim}] AS qv)
            SELECT
                cast(id AS VARCHAR),
                coalesce(nullif(company_name, ''), company, ''),
                coalesce(title, ''),
                coalesce(url, ''),
                coalesce("function", ''),
                coalesce(sub_function, ''),
                coalesce(level, ''),
                coalesce(work_mode, ''),
                coalesce(remote_scope, ''),
                coalesce(country_code, ''),
                coalesce(salary_min_k, -1),
                coalesce(salary_max_k, -1),
                coalesce(location, ''),
                coalesce(city, ''),
                coalesce(region, ''),
                coalesce(role_summary, ''),
                coalesce(jd_markdown, ''),
                greatest(
                    coalesce(array_cosine_similarity(title_embedding::FLOAT[{dim}], q.qv), -1),
                    coalesce(array_cosine_similarity(jd_embedding::FLOAT[{dim}], q.qv), -1),
                    coalesce(list_aggregate(list_transform(
                        alt_titles_embedding,
                        x -> array_cosine_similarity(x::FLOAT[{dim}], q.qv)
                    ), 'max'), -1)
                ) AS similarity
            FROM read_parquet(?), q
            {where_clause}
            ORDER BY similarity DESC
            LIMIT {limit}
            "#
        );

        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(parameters), |row| {
            Ok((job_from_row(row)?, row.get::<_, f32>(17)?))
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Fast semantic search over the attached `sidecar`: rank by cosine over the
    /// truncated `jd_embedding`, then fetch full rows for just the top `limit`.
    /// Hard filters need the dataset's structured columns, so a filtered query
    /// joins the main parquet in the ranking stage; an unfiltered one scans only
    /// the (small) sidecar.
    fn semantic_search_sidecar(
        &self,
        sidecar: &Sidecar,
        query: &[f32],
        criteria: &Criteria,
        limit: usize,
    ) -> Result<Vec<(Job, f32)>> {
        let dim = sidecar.dim;
        // Truncate the query to the sidecar's dimension (Matryoshka prefix);
        // `array_cosine_similarity` renormalizes both sides.
        let literal = query
            .iter()
            .take(dim)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let main = sql_quote(&self.parquet);
        let side = sql_quote(&sidecar.path);

        let mut parameters: Vec<Value> = Vec::new();
        let filters = collect_filters(criteria, &mut parameters);
        let ranked = if filters.is_empty() {
            // Unfiltered: rank straight off the compact sidecar — minimal I/O.
            format!(
                "SELECT cast(id AS VARCHAR) AS rid, \
                        array_cosine_similarity(vec::FLOAT[{dim}], q.qv) AS similarity \
                 FROM read_parquet('{side}'), q \
                 ORDER BY similarity DESC LIMIT {limit}"
            )
        } else {
            // Filtered: prune on the dataset's structured columns first, then rank.
            format!(
                "SELECT cast(m.id AS VARCHAR) AS rid, \
                        array_cosine_similarity(s.vec::FLOAT[{dim}], q.qv) AS similarity \
                 FROM read_parquet('{main}') m \
                 JOIN read_parquet('{side}') s ON cast(m.id AS VARCHAR) = cast(s.id AS VARCHAR), q \
                 WHERE {filters} \
                 ORDER BY similarity DESC LIMIT {limit}",
                filters = filters.join(" AND "),
            )
        };

        let sql = format!(
            r#"
            WITH q AS (SELECT [{literal}]::FLOAT[{dim}] AS qv),
            ranked AS ({ranked})
            SELECT
                cast(m.id AS VARCHAR),
                coalesce(nullif(m.company_name, ''), m.company, ''),
                coalesce(m.title, ''),
                coalesce(m.url, ''),
                coalesce(m."function", ''),
                coalesce(m.sub_function, ''),
                coalesce(m.level, ''),
                coalesce(m.work_mode, ''),
                coalesce(m.remote_scope, ''),
                coalesce(m.country_code, ''),
                coalesce(m.salary_min_k, -1),
                coalesce(m.salary_max_k, -1),
                coalesce(m.location, ''),
                coalesce(m.city, ''),
                coalesce(m.region, ''),
                coalesce(m.role_summary, ''),
                coalesce(m.jd_markdown, ''),
                r.similarity
            FROM ranked r
            JOIN read_parquet('{main}') m ON cast(m.id AS VARCHAR) = r.rid
            ORDER BY r.similarity DESC
            "#
        );

        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(parameters), |row| {
            Ok((job_from_row(row)?, row.get::<_, f32>(17)?))
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// Escapes a string for inlining inside a single-quoted SQL literal (our own
/// config paths, never user input — the query vector and parquet paths are inlined
/// while user-supplied filter values stay parameterized).
fn sql_quote(value: &str) -> String {
    value.replace('\'', "''")
}

/// Builds the shared hard-filter `WHERE` fragments for `criteria`, pushing their
/// bind parameters onto `parameters` (which must already hold the parquet path).
/// Reused by [`JobStore::search`] and [`JobStore::semantic_search`].
fn collect_filters(criteria: &Criteria, parameters: &mut Vec<Value>) -> Vec<String> {
    let mut filters = Vec::new();

    add_exact_filter(
        &mut filters,
        parameters,
        r#"coalesce("function", '')"#,
        &criteria.functions,
    );
    add_exact_filter(
        &mut filters,
        parameters,
        "coalesce(level, '')",
        &criteria.levels,
    );
    add_substring_filter(
        &mut filters,
        parameters,
        "coalesce(title, '')",
        &criteria.titles,
    );
    add_substring_filter(
        &mut filters,
        parameters,
        "concat_ws(' ', company_name, company)",
        &criteria.companies,
    );

    if let Some(country) = criteria.country.as_deref() {
        filters.push(
            "(upper(coalesce(country_code, '')) = upper(?) \
             OR (upper(?) = 'US' \
                 AND lower(coalesce(remote_scope, '')) IN ('us-only', 'us-canada')))"
                .to_owned(),
        );
        parameters.push(Value::Text(country.to_owned()));
        parameters.push(Value::Text(country.to_owned()));
    }

    if !criteria.cities.is_empty() {
        let city_filters = criteria
            .cities
            .iter()
            .map(|city| {
                parameters.push(Value::Text(city.to_lowercase()));
                "contains(lower(concat_ws(' ', city, region, location)), ?)".to_owned()
            })
            .collect::<Vec<_>>();
        filters.push(format!("({})", city_filters.join(" OR ")));
    }

    if let Some(min_comp) = criteria.min_comp {
        filters.push("(coalesce(salary_max_k, -1) = -1 OR salary_max_k >= ?)".to_owned());
        parameters.push(Value::Double(min_comp));
    }

    // Freshness window: only roles posted on or after the threshold. `try_cast`
    // tolerates the varied/empty `posted_at` strings — an unparseable date yields
    // NULL, which fails the comparison, so undated roles are excluded (as intended
    // for "posted in the last N days").
    if let Some(after) = criteria
        .posted_after
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        filters.push("try_cast(posted_at AS TIMESTAMPTZ) >= try_cast(? AS TIMESTAMPTZ)".to_owned());
        parameters.push(Value::Text(after.to_owned()));
    }

    filters
}

/// Parses a comma-delimited float string (from `array_to_string`) into a vector.
fn parse_embedding(packed: &str) -> std::result::Result<Vec<f32>, std::num::ParseFloatError> {
    if packed.is_empty() {
        return Ok(Vec::new());
    }
    packed
        .split(',')
        .map(|value| value.trim().parse())
        .collect()
}

fn add_exact_filter(
    filters: &mut Vec<String>,
    parameters: &mut Vec<Value>,
    column: &str,
    values: &[String],
) {
    if values.is_empty() {
        return;
    }

    filters.push(format!(
        "{column} IN ({})",
        std::iter::repeat_n("?", values.len())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    parameters.extend(values.iter().cloned().map(Value::Text));
}

fn add_substring_filter(
    filters: &mut Vec<String>,
    parameters: &mut Vec<Value>,
    expression: &str,
    values: &[String],
) {
    if values.is_empty() {
        return;
    }

    let value_filters = values
        .iter()
        .map(|value| {
            parameters.push(Value::Text(value.to_lowercase()));
            format!("contains(lower({expression}), ?)")
        })
        .collect::<Vec<_>>();
    filters.push(format!("({})", value_filters.join(" OR ")));
}

fn job_from_row(row: &Row<'_>) -> Result<Job> {
    Ok(Job {
        id: row.get(0)?,
        company: row.get(1)?,
        title: row.get(2)?,
        url: row.get(3)?,
        function: row.get(4)?,
        sub_function: row.get(5)?,
        level: row.get(6)?,
        work_mode: row.get(7)?,
        remote_scope: row.get(8)?,
        country_code: row.get(9)?,
        salary_min_k: row.get(10)?,
        salary_max_k: row.get(11)?,
        location: row.get(12)?,
        city: row.get(13)?,
        region: row.get(14)?,
        role_summary: row.get(15)?,
        jd_markdown: row.get(16)?,
    })
}

#[cfg(test)]
mod tests {
    use super::Criteria;

    #[test]
    fn criteria_is_empty_only_when_no_filter_is_set() {
        assert!(Criteria::default().is_empty());

        assert!(!Criteria {
            cities: vec!["sf".to_owned()],
            ..Default::default()
        }
        .is_empty());
        assert!(!Criteria {
            country: Some("US".to_owned()),
            ..Default::default()
        }
        .is_empty());
        assert!(!Criteria {
            min_comp: Some(150.0),
            ..Default::default()
        }
        .is_empty());
    }
}
