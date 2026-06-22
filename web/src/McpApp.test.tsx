import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MantineProvider } from "@mantine/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { McpApp } from "./McpApp";
import { getJob, rankJobs } from "./api";
import type { Seed } from "./lib";
import type { Job, JobSummary } from "./types";

// The detail + re-rank go through the adapter; the card never searches.
vi.mock("./api", () => ({ getJob: vi.fn(), rankJobs: vi.fn() }));

function role(id: string, title: string): JobSummary {
  return {
    id,
    company: "Acme",
    title,
    location: "San Francisco, CA",
    function: "Engineering",
    level: "Senior",
    remote_scope: "us-only",
    salary_min_k: 150,
    salary_max_k: 200,
    role_summary: "Own the API",
    url: "https://example.com/apply",
  };
}

function seedOf(...titles: [string, string][]): Seed {
  const results = titles.map(([id, title]) => role(id, title));
  return { results: { total: results.length, results } };
}

function renderCard(props: {
  initial: Seed | null;
  subscribe?: (onSeed: (seed: Seed) => void) => void;
  onPreference?: (feedback: { id: string; label: string }[]) => void;
}) {
  return render(
    <MantineProvider>
      <McpApp subscribe={() => {}} {...props} />
    </MantineProvider>,
  );
}

describe("McpApp", () => {
  beforeEach(() => {
    vi.mocked(rankJobs).mockReset();
    vi.mocked(getJob).mockReset();
  });

  it("renders rows from the initial seed (the tool result that opened the app)", () => {
    renderCard({ initial: seedOf(["a", "Backend Engineer"]) });

    expect(screen.getByText("Backend Engineer")).toBeInTheDocument();
    expect(screen.getByText("1 matches")).toBeInTheDocument();
  });

  it("prompts to search in chat when not yet seeded", () => {
    renderCard({ initial: null });
    expect(screen.getByText(/Run a search in the chat/)).toBeInTheDocument();
  });

  it("updates live when Claude pushes a later tool result", () => {
    let push: (seed: Seed) => void = () => {};
    renderCard({ initial: null, subscribe: (onSeed) => (push = onSeed) });
    expect(screen.getByText(/Run a search in the chat/)).toBeInTheDocument();

    act(() => push(seedOf(["a", "Staff Engineer"])));
    expect(screen.getByText("Staff Engineer")).toBeInTheDocument();
  });

  it("surfaces the user's 👍/👎 to Claude's session via onPreference", async () => {
    const onPreference = vi.fn();
    const user = userEvent.setup();
    renderCard({ initial: seedOf(["a", "Backend Engineer"]), onPreference });

    await user.click(screen.getByRole("button", { name: "Like Backend Engineer" }));

    expect(onPreference).toHaveBeenLastCalledWith([{ id: "a", label: "liked" }]);
  });

  it("opens a role's detail inline (scrollable in flow) and returns to the list", async () => {
    const full: Job = {
      ...role("a", "Backend Engineer"),
      sub_function: "Backend",
      work_mode: "remote",
      country_code: "US",
      city: "San Francisco",
      region: "CA",
      jd_markdown: "You will build resilient services.",
    };
    vi.mocked(getJob).mockResolvedValue(full);
    const user = userEvent.setup();
    renderCard({ initial: seedOf(["a", "Backend Engineer"]) });

    await user.click(screen.getByText("Backend Engineer"));
    expect(
      await screen.findByText("You will build resilient services."),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Back to results/ }));
    expect(screen.getByText("1 matches")).toBeInTheDocument();
  });

  it("caps the list to a batch and reveals the rest on request", async () => {
    const many = Array.from(
      { length: 14 },
      (_, i) => [`id${i}`, `Role ${i}`] as [string, string],
    );
    const user = userEvent.setup();
    renderCard({ initial: seedOf(...many) });

    expect(screen.getByText("Role 0")).toBeInTheDocument();
    expect(screen.queryByText("Role 12")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Show all 14/ }));
    expect(screen.getByText("Role 12")).toBeInTheDocument();
  });

  it("re-ranks the card by the user's picks via the taste ranker", async () => {
    // Two roles; liking the second floats it to the top after re-rank.
    vi.mocked(rankJobs).mockResolvedValue({
      results: [
        { id: "b", score: 95, why: "your pick" },
        { id: "a", score: 40, why: "" },
      ],
    });
    const user = userEvent.setup();
    renderCard({ initial: seedOf(["a", "Alpha Eng"], ["b", "Bravo Eng"]) });

    await user.click(screen.getByRole("button", { name: "Like Bravo Eng" }));
    await user.click(screen.getByRole("button", { name: /Re-rank by my picks/ }));

    await waitFor(() => expect(rankJobs).toHaveBeenCalledOnce());
    expect(vi.mocked(rankJobs).mock.calls[0][0]).toMatchObject({
      ids: ["a", "b"],
      feedback: [{ id: "b", label: "liked" }],
    });
    // The score badge from the ranked result renders.
    expect(await screen.findByText("95")).toBeInTheDocument();
  });
});
