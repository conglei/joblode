import { useMemo, useRef, useState } from "react";
import {
  Alert,
  AppShell,
  Center,
  Divider,
  Group,
  Loader,
  Text,
  Title,
} from "@mantine/core";

import { rankJobs, searchJobs, semanticSearch } from "./api";
import { FilterSidebar } from "./components/FilterSidebar";
import { JobDrawer } from "./components/JobDrawer";
import { RankPanel, type RankMethod } from "./components/RankPanel";
import { ResultsTable } from "./components/ResultsTable";
import type {
  FeedbackItem,
  FeedbackLabel,
  Ranked,
  RankParams,
  RankResults,
  SearchParams,
  SearchResults,
} from "./types";

/** The standalone web UI: one search (hard filters + optional semantic query), a
 *  results table, a detail drawer, and a feedback-driven ranking pass — all over
 *  the REST API. The same components serve the MCP App in Phase 5. */
export function App() {
  const [results, setResults] = useState<SearchResults | null>(null);
  // Set when the last search carried a semantic query: similarity by id.
  const [searchScores, setSearchScores] = useState<
    Record<string, Ranked> | undefined
  >(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  // Feedback persists across searches — it's the user's durable taste.
  const [feedback, setFeedback] = useState<Record<string, FeedbackLabel>>({});
  const [ranked, setRanked] = useState<RankResults | null>(null);
  const [rankLoading, setRankLoading] = useState(false);
  const [rankError, setRankError] = useState<string | null>(null);

  // Guards against out-of-order responses: only the most recent call applies.
  const latestSearchId = useRef(0);
  const latestRankId = useRef(0);

  /** One search: semantic (ranked by meaning) when `query` is set, else a plain
   *  hard-filter search. */
  async function runSearch(params: SearchParams, query: string) {
    const searchId = ++latestSearchId.current;
    ++latestRankId.current; // invalidate any in-flight rank call against the old set
    setLoading(true);
    setError(null);
    setRanked(null); // a new result set invalidates the old ranking
    setRankLoading(false);
    setRankError(null);
    try {
      if (query.length > 0) {
        const hits = await semanticSearch({ ...params, query });
        if (searchId !== latestSearchId.current) return;
        setResults({ total: hits.results.length, results: hits.results });
        setSearchScores(
          Object.fromEntries(
            hits.results.map((hit) => [
              hit.id,
              { id: hit.id, score: Math.round(hit.score * 100), why: "" },
            ]),
          ),
        );
      } else {
        const next = await searchJobs(params);
        if (searchId !== latestSearchId.current) return;
        setResults(next);
        setSearchScores(undefined);
      }
    } catch (cause: unknown) {
      if (searchId !== latestSearchId.current) return;
      setError(cause instanceof Error ? cause.message : String(cause));
      setResults(null);
      setSearchScores(undefined);
    } finally {
      if (searchId === latestSearchId.current) setLoading(false);
    }
  }

  function toggleFeedback(id: string, label: FeedbackLabel) {
    setFeedback((current) => {
      const next = { ...current };
      if (next[id] === label) {
        delete next[id]; // clicking the same reaction clears it
      } else {
        next[id] = label;
      }
      return next;
    });
  }

  async function runRank({
    resume,
    method,
  }: {
    resume: string;
    method: RankMethod;
  }) {
    if (!results || results.results.length === 0) return;
    const rankId = ++latestRankId.current;
    setRankLoading(true);
    setRankError(null);

    const feedbackItems: FeedbackItem[] = Object.entries(feedback).map(
      ([id, label]) => ({ id, label }),
    );
    const params: RankParams = {
      ids: results.results.map((row) => row.id),
      top: results.results.length,
    };
    if (feedbackItems.length) params.feedback = feedbackItems;
    if (method !== "free") params.method = method;
    if (resume.trim()) params.resume = resume.trim();

    try {
      const next = await rankJobs(params);
      if (rankId !== latestRankId.current) return;
      setRanked(next);
    } catch (cause: unknown) {
      if (rankId !== latestRankId.current) return;
      setRankError(cause instanceof Error ? cause.message : String(cause));
      setRanked(null);
    } finally {
      if (rankId === latestRankId.current) setRankLoading(false);
    }
  }

  // The ranked view reorders the current rows and attaches scores by id.
  const byId = useMemo(
    () => new Map((results?.results ?? []).map((row) => [row.id, row])),
    [results],
  );
  const rankedRows = ranked
    ? ranked.results
        .map((entry) => byId.get(entry.id))
        .filter((row): row is NonNullable<typeof row> => row !== undefined)
    : null;
  const rankScores = ranked
    ? Object.fromEntries(ranked.results.map((entry) => [entry.id, entry]))
    : undefined;

  const feedbackCount = Object.keys(feedback).length;

  // Ranked reorder takes precedence; otherwise show the search rows (with
  // semantic scores if the search carried a query).
  const tableRows = rankedRows ?? results?.results ?? null;
  const tableScores = ranked ? rankScores : searchScores;

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
        <Divider my="md" />
        <RankPanel
          feedbackCount={feedbackCount}
          loading={rankLoading}
          disabled={!results || results.results.length === 0}
          ranked={ranked !== null}
          onRank={runRank}
          onClear={() => setRanked(null)}
        />
      </AppShell.Navbar>

      <AppShell.Main>
        {error ? (
          <Alert color="red" title="Search failed" mb="md">
            {error}
          </Alert>
        ) : null}
        {rankError ? (
          <Alert color="red" title="Ranking failed" mb="md">
            {rankError}
          </Alert>
        ) : null}
        {ranked && rankedRows ? (
          <Text c="dimmed" mb="xs">
            Ranked {rankedRows.length} of{" "}
            {results?.results.length.toLocaleString()} by your feedback.
          </Text>
        ) : null}

        {loading && !results ? (
          <Center mih={240}>
            <Loader />
          </Center>
        ) : null}
        {tableRows && tableRows.length > 0 ? (
          <ResultsTable
            rows={tableRows}
            scores={tableScores}
            feedback={feedback}
            onFeedback={toggleFeedback}
            onSelect={setSelectedId}
          />
        ) : null}
        {tableRows && tableRows.length === 0 ? (
          <Text c="dimmed">No roles match this search.</Text>
        ) : null}
        {!tableRows && !loading && !error ? (
          <Text c="dimmed">Search to see roles.</Text>
        ) : null}
      </AppShell.Main>

      <JobDrawer jobId={selectedId} onClose={() => setSelectedId(null)} />
    </AppShell>
  );
}
