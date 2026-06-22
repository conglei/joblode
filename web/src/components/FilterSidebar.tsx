import { useState } from "react";
import {
  Button,
  Divider,
  NumberInput,
  Stack,
  TagsInput,
  TextInput,
  Title,
} from "@mantine/core";

import type { SearchParams } from "../types";

/** Drops empty arrays / blank scalars so the request only carries active filters. */
function buildParams(state: {
  titles: string[];
  companies: string[];
  cities: string[];
  functions: string[];
  levels: string[];
  country: string;
  minComp: number | "";
}): SearchParams {
  const params: SearchParams = {};
  if (state.titles.length) params.titles = state.titles;
  if (state.companies.length) params.companies = state.companies;
  if (state.cities.length) params.cities = state.cities;
  if (state.functions.length) params.functions = state.functions;
  if (state.levels.length) params.levels = state.levels;
  // Canonical ISO-2 (the server already matches case-insensitively).
  const country = state.country.trim().toUpperCase();
  if (country) params.country = country;
  if (typeof state.minComp === "number") params.min_comp = state.minComp;
  return params;
}

interface FilterSidebarProps {
  /** Runs a search with the hard filters and an optional semantic `query`. When
   *  `query` is non-empty, results come back ranked by meaning. */
  onSearch: (params: SearchParams, query: string) => void;
  loading: boolean;
}

/** The unified search form: a free-text description (optional — ranks matches by
 *  meaning) plus hard filters. Multi-value fields take free-entry tags (substring
 *  or exact match, per the server); submitting runs one search. */
export function FilterSidebar({ onSearch, loading }: FilterSidebarProps) {
  const [query, setQuery] = useState("");
  const [titles, setTitles] = useState<string[]>([]);
  const [companies, setCompanies] = useState<string[]>([]);
  const [cities, setCities] = useState<string[]>([]);
  const [functions, setFunctions] = useState<string[]>([]);
  const [levels, setLevels] = useState<string[]>([]);
  const [country, setCountry] = useState("");
  const [minComp, setMinComp] = useState<number | "">("");

  function submit() {
    onSearch(
      buildParams({
        titles,
        companies,
        cities,
        functions,
        levels,
        country,
        minComp,
      }),
      query.trim(),
    );
  }

  return (
    <Stack
      gap="sm"
      component="form"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <Title order={4}>Search</Title>
      <TextInput
        label="Describe the role (optional)"
        description="Ranks matches by meaning over the messy fields."
        placeholder="e.g. building data pipelines for ML"
        value={query}
        onChange={(event) => setQuery(event.currentTarget.value)}
      />
      <Divider label="filters" labelPosition="center" />
      <TagsInput
        label="Title"
        placeholder="e.g. backend engineer"
        value={titles}
        onChange={setTitles}
      />
      <TagsInput
        label="Company"
        placeholder="e.g. acme"
        value={companies}
        onChange={setCompanies}
      />
      <TagsInput
        label="City"
        placeholder="e.g. san francisco"
        value={cities}
        onChange={setCities}
      />
      <TagsInput
        label="Function"
        placeholder="e.g. engineering"
        value={functions}
        onChange={setFunctions}
      />
      <TagsInput
        label="Level"
        placeholder="e.g. Senior"
        value={levels}
        onChange={setLevels}
      />
      <TextInput
        label="Country"
        placeholder="ISO-2, e.g. US"
        value={country}
        onChange={(event) => setCountry(event.currentTarget.value)}
      />
      <NumberInput
        label="Min comp (thousands)"
        placeholder="e.g. 150"
        min={0}
        value={minComp}
        onChange={(value) =>
          setMinComp(typeof value === "number" ? value : "")
        }
      />
      <Button type="submit" loading={loading}>
        Search
      </Button>
    </Stack>
  );
}
