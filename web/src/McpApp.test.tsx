import { act, render, screen } from "@testing-library/react";
import { MantineProvider } from "@mantine/core";
import { describe, expect, it, vi } from "vitest";

import { McpApp } from "./McpApp";
import type { Seed } from "./lib";

// The drawer fetches via the adapter; the card itself never searches.
vi.mock("./api", () => ({ getJob: vi.fn() }));

function seedOf(title: string): Seed {
  return {
    results: {
      total: 1,
      results: [
        {
          id: "a",
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
        },
      ],
    },
  };
}

function renderCard(props: {
  initial: Seed | null;
  subscribe: (onSeed: (seed: Seed) => void) => void;
}) {
  return render(
    <MantineProvider>
      <McpApp {...props} />
    </MantineProvider>,
  );
}

describe("McpApp", () => {
  it("renders rows from the initial seed (the tool result that opened the app)", () => {
    renderCard({ initial: seedOf("Backend Engineer"), subscribe: () => {} });

    expect(screen.getByText("Backend Engineer")).toBeInTheDocument();
    expect(screen.getByText("1 matches")).toBeInTheDocument();
  });

  it("prompts to search in chat when not yet seeded", () => {
    renderCard({ initial: null, subscribe: () => {} });

    expect(screen.getByText(/Run a search in the chat/)).toBeInTheDocument();
  });

  it("updates live when Claude pushes a later tool result", () => {
    let push: (seed: Seed) => void = () => {};
    renderCard({
      initial: null,
      subscribe: (onSeed) => {
        push = onSeed;
      },
    });
    expect(screen.getByText(/Run a search in the chat/)).toBeInTheDocument();

    act(() => push(seedOf("Staff Engineer")));

    expect(screen.getByText("Staff Engineer")).toBeInTheDocument();
  });
});
