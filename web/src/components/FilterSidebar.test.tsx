import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MantineProvider } from "@mantine/core";
import { describe, expect, it, vi } from "vitest";

import { FilterSidebar } from "./FilterSidebar";

function renderSidebar(onSearch: (params: unknown, query: string) => void) {
  return render(
    <MantineProvider>
      <FilterSidebar onSearch={onSearch} loading={false} />
    </MantineProvider>,
  );
}

async function typeInto(
  user: ReturnType<typeof userEvent.setup>,
  placeholder: string,
  value: string,
) {
  // Query by placeholder: Mantine's TagsInput exposes two inputs per label.
  await user.type(screen.getByPlaceholderText(placeholder), value);
}

describe("FilterSidebar", () => {
  it("submits only the filled filters, projected onto SearchParams", async () => {
    const user = userEvent.setup();
    const onSearch = vi.fn();
    renderSidebar(onSearch);

    await typeInto(user, "e.g. backend engineer", "backend{Enter}");
    await typeInto(user, "e.g. acme", "acme{Enter}");
    await typeInto(user, "e.g. san francisco", "sf{Enter}");
    await typeInto(user, "e.g. engineering", "engineering{Enter}");
    await typeInto(user, "e.g. Senior", "Senior{Enter}");
    await typeInto(user, "ISO-2, e.g. US", "US");
    await typeInto(user, "e.g. 150", "150");
    // A semantic description rides along with the same filters.
    await typeInto(user, "e.g. building data pipelines for ML", "ml work");

    await user.click(screen.getByRole("button", { name: "Search" }));

    expect(onSearch).toHaveBeenCalledWith(
      {
        titles: ["backend"],
        companies: ["acme"],
        cities: ["sf"],
        functions: ["engineering"],
        levels: ["Senior"],
        country: "US",
        min_comp: 150,
      },
      "ml work",
    );
  });

  it("submits an empty object and blank query when nothing is set", async () => {
    const user = userEvent.setup();
    const onSearch = vi.fn();
    renderSidebar(onSearch);

    await user.click(screen.getByRole("button", { name: "Search" }));

    expect(onSearch).toHaveBeenCalledWith({}, "");
  });
});
