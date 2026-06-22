import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MantineProvider } from "@mantine/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";
import { getJob, rankJobs, searchJobs, semanticSearch } from "./api";
import type { Job, RankResults, SearchResults } from "./types";

vi.mock("./api", () => ({
  searchJobs: vi.fn(),
  getJob: vi.fn(),
  rankJobs: vi.fn(),
  semanticSearch: vi.fn(),
}));

const summary = {
  id: "city-direct",
  company: "Acme",
  title: "Backend Engineer",
  location: "San Francisco, CA",
  function: "Engineering",
  level: "Senior",
  remote_scope: "us-only",
  salary_min_k: 150,
  salary_max_k: 200,
  role_summary: "Own the API",
  url: "https://example.com/apply",
};

const results: SearchResults = { total: 1, results: [summary] };
const job: Job = {
  ...summary,
  sub_function: "Backend",
  work_mode: "remote",
  country_code: "US",
  city: "San Francisco",
  region: "CA",
  jd_markdown: "# Backend Engineer\n\nYou will build resilient services.",
};

function renderApp() {
  return render(
    <MantineProvider>
      <App />
    </MantineProvider>,
  );
}

describe("App", () => {
  beforeEach(() => {
    vi.mocked(searchJobs).mockReset().mockResolvedValue(results);
    vi.mocked(getJob).mockReset().mockResolvedValue(job);
    vi.mocked(rankJobs).mockReset();
    vi.mocked(semanticSearch).mockReset();
  });

  it("searches, lists rows, and opens a role's detail drawer", async () => {
    const user = userEvent.setup();
    renderApp();

    await user.click(screen.getByRole("button", { name: "Search" }));

    // The search ran and the row rendered.
    expect(searchJobs).toHaveBeenCalledOnce();
    const row = await screen.findByText("Backend Engineer");
    expect(screen.getByText("1 matches")).toBeInTheDocument();

    // Clicking the row fetches the full record and shows the JD in the drawer.
    await user.click(row);
    await waitFor(() => expect(getJob).toHaveBeenCalledWith("city-direct"));
    expect(
      await screen.findByText("You will build resilient services."),
    ).toBeInTheDocument();
  });

  it("shows an empty state when nothing matches", async () => {
    vi.mocked(searchJobs).mockResolvedValue({ total: 0, results: [] });
    const user = userEvent.setup();
    renderApp();

    await user.click(screen.getByRole("button", { name: "Search" }));
    expect(
      await screen.findByText("No roles match this search."),
    ).toBeInTheDocument();
  });

  it("surfaces a search failure", async () => {
    vi.mocked(searchJobs).mockRejectedValue(new Error("boom"));
    const user = userEvent.setup();
    renderApp();

    await user.click(screen.getByRole("button", { name: "Search" }));
    expect(await screen.findByText("Search failed")).toBeInTheDocument();
    expect(screen.getByText("boom")).toBeInTheDocument();
  });

  it("surfaces a get_job failure in the drawer", async () => {
    vi.mocked(getJob).mockRejectedValue(new Error("drawer boom"));
    const user = userEvent.setup();
    renderApp();

    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.click(await screen.findByText("Backend Engineer"));
    expect(await screen.findByText("drawer boom")).toBeInTheDocument();
  });

  it("ranks the results from feedback and shows scores", async () => {
    const ranked: RankResults = {
      results: [{ id: "city-direct", score: 88, why: "strong backend fit" }],
    };
    vi.mocked(rankJobs).mockResolvedValue(ranked);
    const user = userEvent.setup();
    renderApp();

    await user.click(screen.getByRole("button", { name: "Search" }));
    await screen.findByText("Backend Engineer");

    // React to the role, then rank.
    await user.click(
      screen.getByRole("button", { name: "Like Backend Engineer" }),
    );
    await user.click(screen.getByRole("button", { name: "Rank results" }));

    // The rank call carried the candidate ids and the feedback signal.
    await waitFor(() => expect(rankJobs).toHaveBeenCalledOnce());
    expect(vi.mocked(rankJobs).mock.calls[0][0]).toMatchObject({
      ids: ["city-direct"],
      feedback: [{ id: "city-direct", label: "liked" }],
    });

    // The score badge renders in the ranked view.
    expect(await screen.findByText("88")).toBeInTheDocument();
    expect(screen.getByText(/Ranked 1 of 1/)).toBeInTheDocument();
  });

  it("runs a semantic search from the unified form when a query is given", async () => {
    vi.mocked(semanticSearch).mockResolvedValue({
      results: [{ ...summary, score: 0.91 }],
    });
    const user = userEvent.setup();
    renderApp();

    await user.type(
      screen.getByLabelText("Describe the role (optional)"),
      "resilient backend services",
    );
    await user.click(screen.getByRole("button", { name: "Search" }));

    // A query routes the single Search through semantic, not plain search.
    await waitFor(() => expect(semanticSearch).toHaveBeenCalledOnce());
    expect(searchJobs).not.toHaveBeenCalled();
    expect(vi.mocked(semanticSearch).mock.calls[0][0]).toMatchObject({
      query: "resilient backend services",
    });

    // The hit renders with its similarity scaled to 0–100.
    expect(await screen.findByText("Backend Engineer")).toBeInTheDocument();
    expect(screen.getByText("91")).toBeInTheDocument();
  });

  it("surfaces a ranking failure (e.g. unconfigured model)", async () => {
    vi.mocked(rankJobs).mockRejectedValue(
      new Error("ranking method 'match' requires a configured model"),
    );
    const user = userEvent.setup();
    renderApp();

    await user.click(screen.getByRole("button", { name: "Search" }));
    await screen.findByText("Backend Engineer");
    await user.click(screen.getByRole("button", { name: "Rank results" }));

    expect(await screen.findByText("Ranking failed")).toBeInTheDocument();
  });
});
