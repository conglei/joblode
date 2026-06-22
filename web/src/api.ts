/** HTTP data source: talks to the Rust server's REST API (`/api`). This is the
 *  standalone-web-app runtime; the MCP App bridge runtime arrives in Phase 5
 *  (see `docs/DESIGN.md` §7), and both will share these return types. */

import type {
  Job,
  RankParams,
  RankResults,
  SearchParams,
  SearchResults,
  SemanticParams,
  SemanticResults,
} from "./types";

/** Runs a hard-filter search. Throws on a non-2xx response. */
export async function searchJobs(params: SearchParams): Promise<SearchResults> {
  const response = await fetch("/api/search", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params),
  });
  if (!response.ok) {
    throw new Error(`search failed (${response.status})`);
  }
  return response.json() as Promise<SearchResults>;
}

/** Fetches one role's full record, including `jd_markdown`. Throws on non-2xx. */
export async function getJob(id: string): Promise<Job> {
  const response = await fetch(`/api/job/${encodeURIComponent(id)}`);
  if (!response.ok) {
    throw new Error(`get_job failed (${response.status})`);
  }
  return response.json() as Promise<Job>;
}

/** Ranks a candidate set into a compact shortlist. The 400 from a model method
 *  without a configured key surfaces as a readable message. Throws on non-2xx. */
export async function rankJobs(params: RankParams): Promise<RankResults> {
  const response = await fetch("/api/rank", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params),
  });
  if (!response.ok) {
    const detail = (await response.text()).trim();
    throw new Error(detail || `rank failed (${response.status})`);
  }
  return response.json() as Promise<RankResults>;
}

/** Semantic search over role embeddings. The 400 from a missing embeddings key
 *  surfaces as a readable message. Throws on non-2xx. */
export async function semanticSearch(
  params: SemanticParams,
): Promise<SemanticResults> {
  const response = await fetch("/api/semantic", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params),
  });
  if (!response.ok) {
    const detail = (await response.text()).trim();
    throw new Error(detail || `semantic search failed (${response.status})`);
  }
  return response.json() as Promise<SemanticResults>;
}
