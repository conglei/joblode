import { afterEach, describe, expect, it, vi } from "vitest";

import {
  createBridgeSource,
  getJob,
  httpSource,
  inMcpApp,
  rankJobs,
  search,
  setActiveSource,
  type ToolBridge,
} from "./api";

function mockFetch(body: unknown, ok = true, status = 200) {
  const fetchMock = vi.fn().mockResolvedValue({
    ok,
    status,
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(typeof body === "string" ? body : ""),
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

afterEach(() => {
  vi.unstubAllGlobals();
  setActiveSource(httpSource); // a swap test must not leak into the next test
});

/** A fake App bridge that records the call and returns a canned tool result. */
function fakeBridge(result: {
  structuredContent?: unknown;
  content?: { type: string; text?: string }[];
  isError?: boolean;
}): ToolBridge & { calls: { name: string; arguments: unknown }[] } {
  const calls: { name: string; arguments: unknown }[] = [];
  return {
    calls,
    callServerTool: (request) => {
      calls.push(request);
      return Promise.resolve(result);
    },
  };
}

describe("search", () => {
  it("POSTs filters (and an optional query) as JSON and returns the results", async () => {
    const results = { total: 0, results: [] };
    const fetchMock = mockFetch(results);

    expect(await search({ cities: ["sf"], query: "ml pipelines" })).toEqual(
      results,
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/search",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ cities: ["sf"], query: "ml pipelines" }),
      }),
    );
  });

  it("surfaces the server's error (e.g. a query with no embeddings key)", async () => {
    mockFetch("a semantic query requires a configured embeddings model", false, 400);
    await expect(search({ query: "x" })).rejects.toThrow(
      "requires a configured embeddings model",
    );
  });
});

describe("getJob", () => {
  it("GETs by url-encoded id and returns the record", async () => {
    const job = { id: "a b" };
    const fetchMock = mockFetch(job);

    expect(await getJob("a b")).toEqual(job);
    expect(fetchMock).toHaveBeenCalledWith("/api/job/a%20b");
  });

  it("throws on a non-2xx response", async () => {
    mockFetch({}, false, 404);
    await expect(getJob("x")).rejects.toThrow("get_job failed (404)");
  });
});

describe("rankJobs", () => {
  it("POSTs the rank params as JSON and returns the shortlist", async () => {
    const ranked = { results: [{ id: "a", score: 90, why: "fit" }] };
    const fetchMock = mockFetch(ranked);

    const params = {
      ids: ["a"],
      feedback: [{ id: "a", label: "liked" as const }],
    };
    expect(await rankJobs(params)).toEqual(ranked);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/rank",
      expect.objectContaining({ method: "POST", body: JSON.stringify(params) }),
    );
  });

  it("surfaces the server's error message (e.g. unconfigured model)", async () => {
    mockFetch("ranking method 'match' requires a configured model", false, 400);
    await expect(rankJobs({ method: "match" })).rejects.toThrow(
      "requires a configured model",
    );
  });
});

describe("createBridgeSource", () => {
  it("calls the named tool with the params and returns its structured content", async () => {
    const results = { total: 1, results: [{ id: "a" }] };
    const bridge = fakeBridge({ structuredContent: results });
    const source = createBridgeSource(bridge);

    expect(await source.search({ cities: ["sf"] })).toEqual(results);
    expect(bridge.calls).toEqual([
      { name: "search", arguments: { cities: ["sf"] } },
    ]);
  });

  it("wraps get_job's id into the tool arguments", async () => {
    const bridge = fakeBridge({ structuredContent: { id: "a b" } });
    const source = createBridgeSource(bridge);

    await source.getJob("a b");
    expect(bridge.calls).toEqual([{ name: "get_job", arguments: { id: "a b" } }]);
  });

  it("throws the tool's error text when the result is an error", async () => {
    const bridge = fakeBridge({
      isError: true,
      content: [{ type: "text", text: "ranking method 'match' requires a key" }],
    });
    const source = createBridgeSource(bridge);

    await expect(source.rankJobs({ method: "match" })).rejects.toThrow(
      "requires a key",
    );
  });

  it("throws when a result carries no structured content", async () => {
    const source = createBridgeSource(fakeBridge({}));
    await expect(source.search({ query: "x" })).rejects.toThrow("search failed");
  });
});

describe("source selection", () => {
  it("defaults to the HTTP source and runs outside an MCP App", () => {
    // jsdom's window is its own top, so we are not embedded.
    expect(inMcpApp()).toBe(false);
  });

  it("setActiveSource routes the exported calls to the chosen source", async () => {
    const results = { total: 0, results: [] };
    const bridge = fakeBridge({ structuredContent: results });
    setActiveSource(createBridgeSource(bridge));

    expect(await search({ titles: ["eng"] })).toEqual(results);
    expect(bridge.calls[0]).toEqual({
      name: "search",
      arguments: { titles: ["eng"] },
    });
  });
});
