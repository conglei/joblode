import type { JobSummary, Ranked, SearchResults } from "./types";

/** Format a salary range given in thousands. Returns "" when both ends are unknown. */
export function formatSalary(minK: number, maxK: number): string {
  if (minK <= 0 && maxK <= 0) return "";
  const lo = minK > 0 ? `${Math.round(minK)}` : "?";
  const hi = maxK > 0 ? `${Math.round(maxK)}` : "?";
  return `$${lo}–${hi}k`;
}

/** Initial table state seeded from a tool result. */
export interface Seed {
  results: SearchResults;
  /** Per-id scores when the result carried them (a semantic search). */
  scores?: Record<string, Ranked>;
}

/** Maps a tool result's `structuredContent` to seed state for the first render in
 *  an MCP App — when Claude calls `search_jobs`/`semantic_search`, the host pushes
 *  the result and the table shows it immediately. Returns `null` when there's
 *  nothing renderable: a `rank_jobs` result (ids/scores, no rows), an empty set,
 *  or an unexpected shape — the app then just shows its "search to see roles" state. */
export function seedFromToolResult(structured: unknown): Seed | null {
  if (structured === null || typeof structured !== "object") return null;
  const obj = structured as { total?: unknown; results?: unknown };
  if (!Array.isArray(obj.results) || obj.results.length === 0) return null;

  const raw = obj.results as Record<string, unknown>[];
  // Only `search`/`semantic` rows carry the fields the table renders; a rank
  // result has just {id, score, why}, so it can't seed a standalone table.
  const renderable = raw.every(
    (row) =>
      typeof row.id === "string" &&
      typeof row.company === "string" &&
      typeof row.title === "string",
  );
  if (!renderable) return null;

  const rows = raw as unknown as JobSummary[];
  const results: SearchResults = {
    total: typeof obj.total === "number" ? obj.total : rows.length,
    results: rows,
  };

  // A semantic search carries a similarity per row; surface it as a score.
  if (raw.every((row) => typeof row.score === "number")) {
    const scores: Record<string, Ranked> = Object.fromEntries(
      raw.map((row) => [
        row.id as string,
        { id: row.id as string, score: Math.round((row.score as number) * 100), why: "" },
      ]),
    );
    return { results, scores };
  }
  return { results };
}
