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

/// Read-only access to one parquet dataset.
pub struct JobStore {
    connection: Connection,
    parquet: String,
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
        })
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

        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        // coalesce: rows with a NULL `jd_embedding` come back as "" (→ empty vec),
        // never as a NULL that would fail the string conversion.
        let sql = format!(
            "SELECT cast(id AS VARCHAR), coalesce(array_to_string(jd_embedding, ','), '') \
             FROM read_parquet(?) \
             WHERE cast(id AS VARCHAR) IN ({placeholders})"
        );

        let mut parameters: Vec<Value> = Vec::with_capacity(ids.len() + 1);
        parameters.push(Value::Text(self.parquet.clone()));
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
