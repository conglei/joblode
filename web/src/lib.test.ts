import { describe, expect, it } from "vitest";
import { formatSalary, seedFromToolResult } from "./lib";

describe("formatSalary", () => {
  it("formats a known range", () => {
    expect(formatSalary(120, 180)).toBe("$120–180k");
  });

  it("uses ? for an unknown end", () => {
    expect(formatSalary(120, -1)).toBe("$120–?k");
  });

  it("returns empty when both ends are unknown", () => {
    expect(formatSalary(-1, -1)).toBe("");
  });
});

const row = {
  id: "a",
  company: "Acme",
  title: "Backend Engineer",
  location: "SF",
  function: "Engineering",
  level: "Senior",
  remote_scope: "us-only",
  salary_min_k: 150,
  salary_max_k: 200,
  role_summary: "Own the API",
  url: "https://x/apply",
};

describe("seedFromToolResult", () => {
  it("seeds a search result, defaulting total to the row count when absent", () => {
    expect(seedFromToolResult({ total: 42, results: [row] })).toEqual({
      results: { total: 42, results: [row] },
    });
    expect(seedFromToolResult({ results: [row] })?.results.total).toBe(1);
  });

  it("carries per-id scores from a semantic result, scaled to 0–100", () => {
    const seed = seedFromToolResult({ results: [{ ...row, score: 0.91 }] });
    expect(seed?.scores).toEqual({ a: { id: "a", score: 91, why: "" } });
  });

  it("returns null for a rank result (no rows to render)", () => {
    expect(
      seedFromToolResult({ results: [{ id: "a", score: 88, why: "fit" }] }),
    ).toBeNull();
  });

  it("returns null for an empty, missing, or non-object result", () => {
    expect(seedFromToolResult({ total: 0, results: [] })).toBeNull();
    expect(seedFromToolResult({})).toBeNull();
    expect(seedFromToolResult(null)).toBeNull();
    expect(seedFromToolResult(undefined)).toBeNull();
  });
});
