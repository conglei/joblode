/** HTTP data source: talks to the Rust server's REST API (`/api`). This is the
 *  standalone-web-app runtime; the MCP App bridge runtime arrives in Phase 5
 *  (see `docs/DESIGN.md` §7), and both will share these return types. */

import type { Job, SearchParams, SearchResults } from "./types";

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
