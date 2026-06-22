import { useState } from "react";
import {
  Alert,
  AppShell,
  Center,
  Group,
  Loader,
  Text,
  Title,
} from "@mantine/core";

import { searchJobs } from "./api";
import { FilterSidebar } from "./components/FilterSidebar";
import { JobDrawer } from "./components/JobDrawer";
import { ResultsTable } from "./components/ResultsTable";
import type { SearchParams, SearchResults } from "./types";

/** The standalone web UI: filter sidebar, results table, and a detail drawer,
 *  over the REST API. The same components serve the MCP App in Phase 5. */
export function App() {
  const [results, setResults] = useState<SearchResults | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  async function runSearch(params: SearchParams) {
    setLoading(true);
    setError(null);
    try {
      setResults(await searchJobs(params));
    } catch (cause: unknown) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setResults(null);
    } finally {
      setLoading(false);
    }
  }

  return (
    <AppShell
      header={{ height: 56 }}
      navbar={{ width: 300, breakpoint: "sm" }}
      padding="md"
    >
      <AppShell.Header>
        <Group h="100%" px="md" justify="space-between">
          <Title order={3}>joblode</Title>
          {results ? (
            <Text c="dimmed">{results.total.toLocaleString()} matches</Text>
          ) : null}
        </Group>
      </AppShell.Header>

      <AppShell.Navbar p="md">
        <FilterSidebar onSearch={runSearch} loading={loading} />
      </AppShell.Navbar>

      <AppShell.Main>
        {error ? (
          <Alert color="red" title="Search failed">
            {error}
          </Alert>
        ) : null}
        {loading && !results ? (
          <Center mih={240}>
            <Loader />
          </Center>
        ) : null}
        {results && results.results.length > 0 ? (
          <ResultsTable rows={results.results} onSelect={setSelectedId} />
        ) : null}
        {results && results.results.length === 0 ? (
          <Text c="dimmed">No roles match these filters.</Text>
        ) : null}
        {!results && !loading && !error ? (
          <Text c="dimmed">Set filters and search to see roles.</Text>
        ) : null}
      </AppShell.Main>

      <JobDrawer jobId={selectedId} onClose={() => setSelectedId(null)} />
    </AppShell>
  );
}
