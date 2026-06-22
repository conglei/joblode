/** Wire types mirroring the Rust `dto` / `joblode_core::Job`. Keep in sync with
 *  `crates/joblode-server/src/dto.rs`. */

/** Hard search filters plus a row cap. Every field is optional. */
export interface SearchParams {
  functions?: string[];
  levels?: string[];
  titles?: string[];
  companies?: string[];
  cities?: string[];
  country?: string | null;
  min_comp?: number | null;
  limit?: number;
}

/** Token-shaped search row — enough to triage, no full description. */
export interface JobSummary {
  id: string;
  company: string;
  title: string;
  location: string;
  function: string;
  level: string;
  remote_scope: string;
  salary_min_k: number;
  salary_max_k: number;
  role_summary: string;
  url: string;
}

/** `search` result: the full match count plus a capped page of rows. */
export interface SearchResults {
  total: number;
  results: JobSummary[];
}

/** The full record returned by `get_job`, including the description. */
export interface Job extends JobSummary {
  sub_function: string;
  work_mode: string;
  country_code: string;
  city: string;
  region: string;
  jd_markdown: string;
}
