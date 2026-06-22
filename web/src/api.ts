/** The data-source seam (DESIGN §7). Components call `searchJobs`/`getJob`/… and
 *  never know whether the data comes over HTTP (the standalone web app) or the MCP
 *  App bridge (inside Claude). Both runtimes implement {@link DataSource}; the
 *  active one is selected at boot by `main.tsx` via {@link setActiveSource}. */

import type {
  Job,
  RankParams,
  RankResults,
  SearchParams,
  SearchResults,
} from "./types";

/** The three reads every runtime provides, on identical request/response shapes. */
export interface DataSource {
  search(params: SearchParams): Promise<SearchResults>;
  getJob(id: string): Promise<Job>;
  rankJobs(params: RankParams): Promise<RankResults>;
}

// — HTTP runtime: talks to the Rust server's REST API (`/api`). ———————————————

/** Runs one search — filters, plus a semantic `query` when set. A 400 (e.g. a
 *  query with no embeddings key) surfaces as a readable message. Throws on non-2xx. */
async function httpSearch(params: SearchParams): Promise<SearchResults> {
  const response = await fetch("/api/search", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params),
  });
  if (!response.ok) {
    const detail = (await response.text()).trim();
    throw new Error(detail || `search failed (${response.status})`);
  }
  return response.json() as Promise<SearchResults>;
}

/** Fetches one role's full record, including `jd_markdown`. Throws on non-2xx. */
async function httpGetJob(id: string): Promise<Job> {
  const response = await fetch(`/api/job/${encodeURIComponent(id)}`);
  if (!response.ok) {
    throw new Error(`get_job failed (${response.status})`);
  }
  return response.json() as Promise<Job>;
}

/** Ranks a candidate set into a compact shortlist. The 400 from a model method
 *  without a configured key surfaces as a readable message. Throws on non-2xx. */
async function httpRankJobs(params: RankParams): Promise<RankResults> {
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

/** The default runtime: the REST adapter. */
export const httpSource: DataSource = {
  search: httpSearch,
  getJob: httpGetJob,
  rankJobs: httpRankJobs,
};

// — MCP App bridge runtime: calls our tools over postMessage (inside Claude). ——

/** The slice of the `@modelcontextprotocol/ext-apps` `App` we depend on: calling a
 *  server tool by name. Kept structural so the real `App` satisfies it and tests
 *  can pass a fake — no host required. */
export interface ToolBridge {
  callServerTool(request: {
    name: string;
    arguments: Record<string, unknown>;
  }): Promise<ToolCallResult>;
}

/** The MCP tool-call envelope we read: structured JSON is the payload (DESIGN §4),
 *  with `content` text used only to surface an error. */
interface ToolCallResult {
  structuredContent?: unknown;
  content?: { type: string; text?: string }[];
  isError?: boolean;
}

/** Unwraps a tool result to its structured payload, or throws the error text. */
function unwrap<T>(result: ToolCallResult, tool: string): T {
  if (result.isError || result.structuredContent === undefined) {
    const message = result.content?.find((c) => c.type === "text")?.text;
    throw new Error(message?.trim() || `${tool} failed`);
  }
  return result.structuredContent as T;
}

/** A {@link DataSource} backed by the MCP App bridge: each read is one `tools/call`
 *  to the host, returning the tool's structured JSON. The tool names and argument
 *  shapes match the REST routes, so `types.ts` serves both. */
export function createBridgeSource(bridge: ToolBridge): DataSource {
  const call = async <T>(
    name: string,
    args: object,
  ): Promise<T> => {
    const result = await bridge.callServerTool({
      name,
      arguments: { ...args } as Record<string, unknown>,
    });
    return unwrap<T>(result, name);
  };
  return {
    search: (params) => call("search", params),
    getJob: (id) => call("get_job", { id }),
    rankJobs: (params) => call("rank_jobs", params),
  };
}

/** True when running inside an MCP App host (a sandboxed iframe), as opposed to
 *  the standalone web app. Boot wiring (`main.tsx`) uses this to pick the bridge. */
export function inMcpApp(): boolean {
  return typeof window !== "undefined" && window.self !== window.top;
}

// — Active source: components dispatch through here; boot swaps it once. ————————

let active: DataSource = httpSource;

/** Installs the runtime data source. Called once at boot before first render. */
export function setActiveSource(source: DataSource): void {
  active = source;
}

export const search = (params: SearchParams) => active.search(params);
export const getJob = (id: string) => active.getJob(id);
export const rankJobs = (params: RankParams) => active.rankJobs(params);
