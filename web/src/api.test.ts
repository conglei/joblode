import { afterEach, describe, expect, it, vi } from "vitest";

import { getJob, searchJobs } from "./api";

function mockFetch(body: unknown, ok = true, status = 200) {
  const fetchMock = vi.fn().mockResolvedValue({
    ok,
    status,
    json: () => Promise.resolve(body),
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
