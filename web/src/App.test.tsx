import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MantineProvider } from "@mantine/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";
import { getJob, searchJobs } from "./api";
import type { Job, SearchResults } from "./types";

vi.mock("./api", () => ({
  searchJobs: vi.fn(),
  getJob: vi.fn(),
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
      await screen.findByText("No roles match these filters."),
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
});
