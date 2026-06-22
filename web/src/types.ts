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

/** A user's reaction to a role — the feedback-loop signal. */
export type FeedbackLabel = "liked" | "disliked";

/** One reaction passed to `rank` to personalize the free taste ranking. */
export interface FeedbackItem {
  id: string;
  label: FeedbackLabel;
}

/** `rank` input: a candidate source (filters or explicit `ids`), the resume +
 *  method for the optional model pass, and prior feedback. */
export interface RankParams extends SearchParams {
  ids?: string[];
  resume?: string;
  feedback?: FeedbackItem[];
  /** Omit for the free, keyless taste ranking. */
  method?: "match" | "pairwise";
  top?: number;
}

/** One ranked role: id + 0–100 score + an optional one-line reason. */
export interface Ranked {
  id: string;
  score: number;
  why: string;
}

/** `rank` result: a compact, ordered shortlist. */
export interface RankResults {
  results: Ranked[];
}

/** `semantic` input: a free-text query plus the same hard filters. */
export interface SemanticParams extends SearchParams {
  query: string;
}

/** One semantic hit: a compact row plus its cosine similarity in [-1, 1]. */
export interface SemanticHit extends JobSummary {
  score: number;
}

/** `semantic` result: rows ordered by similarity, best first. */
export interface SemanticResults {
  results: SemanticHit[];
}
