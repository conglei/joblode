import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MantineProvider } from "@mantine/core";
import { describe, expect, it, vi } from "vitest";

import { ResultsTable } from "./ResultsTable";
import type { JobSummary } from "../types";

const base: JobSummary = {
  id: "with-comp",
  company: "Acme",
  title: "Backend Engineer",
  location: "SF",
  function: "Engineering",
  level: "Senior",
  remote_scope: "",
  salary_min_k: 150,
  salary_max_k: 200,
  role_summary: "",
  url: "https://example.com/apply",
};

const rows: JobSummary[] = [
  base,
  // Unknown comp exercises the "—" fallback; no level exercises the badge skip.
  {
    ...base,
    id: "no-comp",
    title: "Data Analyst",
    level: "",
    salary_min_k: -1,
    salary_max_k: -1,
  },
];

function renderTable(onSelect: (id: string) => void) {
  return render(
    <MantineProvider>
      <ResultsTable rows={rows} onSelect={onSelect} />
    </MantineProvider>,
  );
}

describe("ResultsTable", () => {
  it("selects a role when its row is clicked", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    renderTable(onSelect);

    await user.click(screen.getByText("Backend Engineer"));
    expect(onSelect).toHaveBeenCalledWith("with-comp");
  });

  it("selects a focused row on Enter", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    renderTable(onSelect);

    screen.getByText("Backend Engineer").closest("tr")?.focus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledWith("with-comp");
  });

  it("renders an em dash for unknown comp", () => {
    renderTable(vi.fn());
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("opens the apply link without selecting the row", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    renderTable(onSelect);

    const link = screen.getAllByRole("link", { name: "Open" })[0];
    expect(link).toHaveAttribute("href", "https://example.com/apply");
    await user.click(link);
    expect(onSelect).not.toHaveBeenCalled();
  });
});
