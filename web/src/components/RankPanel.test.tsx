import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MantineProvider } from "@mantine/core";
import { describe, expect, it, vi } from "vitest";

import { RankPanel } from "./RankPanel";

function renderPanel(props: Partial<Parameters<typeof RankPanel>[0]> = {}) {
  const onRank = vi.fn();
  const onClear = vi.fn();
  render(
    <MantineProvider>
      <RankPanel
        feedbackCount={0}
        loading={false}
        disabled={false}
        ranked={false}
        onRank={onRank}
        onClear={onClear}
        {...props}
      />
    </MantineProvider>,
  );
  return { onRank, onClear };
}

describe("RankPanel", () => {
  it("reveals the resume field for match and passes resume + method", async () => {
    const user = userEvent.setup();
    const { onRank } = renderPanel();

    // Resume is hidden for the free method.
    expect(screen.queryByLabelText("Resume")).not.toBeInTheDocument();

    await user.click(screen.getByText("Match"));
    await user.type(screen.getByLabelText("Resume"), "15 years backend");
    await user.click(screen.getByRole("button", { name: "Rank results" }));

    expect(onRank).toHaveBeenCalledWith({
      resume: "15 years backend",
      method: "match",
    });
  });

  it("shows a Clear button once ranked", async () => {
    const user = userEvent.setup();
    const { onClear } = renderPanel({ ranked: true });

    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(onClear).toHaveBeenCalledOnce();
  });

  it("disables ranking when there is nothing to rank", () => {
    renderPanel({ disabled: true });
    expect(screen.getByRole("button", { name: "Rank results" })).toBeDisabled();
  });
});
