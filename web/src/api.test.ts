import { afterEach, describe, expect, it, vi } from "vitest";

import { getJob, rankJobs, searchJobs, semanticSearch } from "./api";

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
});

describe("searchJobs", () => {
  it("POSTs the criteria as JSON and returns the results", async () => {
    const results = { total: 0, results: [] };
    const fetchMock = mockFetch(results);

    expect(await searchJobs({ cities: ["sf"] })).toEqual(results);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/search",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ cities: ["sf"] }),
      }),
    );
  });

  it("throws on a non-2xx response", async () => {
    mockFetch({}, false, 500);
    await expect(searchJobs({})).rejects.toThrow("search failed (500)");
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

describe("semanticSearch", () => {
  it("POSTs the query + filters and returns hits", async () => {
    const hits = { results: [{ id: "a", score: 0.91 }] };
    const fetchMock = mockFetch(hits);

    const params = { query: "ml pipelines", functions: ["data"] };
    expect(await semanticSearch(params)).toEqual(hits);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/semantic",
      expect.objectContaining({ method: "POST", body: JSON.stringify(params) }),
    );
  });

  it("surfaces the server's error (e.g. no embeddings key)", async () => {
    mockFetch("semantic search requires a configured embeddings model", false, 400);
    await expect(semanticSearch({ query: "x" })).rejects.toThrow(
      "requires a configured embeddings model",
    );
  });
});
