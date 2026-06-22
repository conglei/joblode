import { useEffect, useMemo, useState } from "react";
import { Alert, Anchor, Box, Button, Group, Text, Title } from "@mantine/core";

import { rankJobs } from "./api";
import { JobDetail } from "./components/JobDetail";
import { ResultsTable } from "./components/ResultsTable";
import type { Seed } from "./lib";
import type { FeedbackItem, FeedbackLabel, RankResults } from "./types";

/** How many roles to show before a "show all" — small enough to triage at a glance. */
const BATCH = 10;

interface McpAppProps {
  /** Seed captured before mount (the tool result that opened the app), if any. */
  initial: Seed | null;
  /** Registers a callback for tool results pushed after mount (a later search). */
  subscribe: (onSeed: (seed: Seed) => void) => void;
  /** Pushes the running 👍/👎 preference into Claude's session (the host persists
   *  it and can reuse it in future searches). No-op standalone / in tests. */
  onPreference?: (feedback: FeedbackItem[]) => void;
}

/** The embedded MCP App card (DESIGN §7/§13): renders the roles Claude found and
 *  lets the user validate them with 👍/👎. Reactions are surfaced to Claude (so the
 *  preference lives in the session, §13) and re-rank the card in place via the
 *  keyless taste ranker (§6). Claude drives the search; there's no filter sidebar. */
export function McpApp({ initial, subscribe, onPreference }: McpAppProps) {
  const [seed, setSeed] = useState<Seed | null>(initial);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<Record<string, FeedbackLabel>>({});
  const [ranked, setRanked] = useState<RankResults | null>(null);
  const [rankError, setRankError] = useState<string | null>(null);
  const [rankLoading, setRankLoading] = useState(false);
  const [showAll, setShowAll] = useState(false);

  // A later search Claude runs replaces the set, so the old ranking is stale.
  useEffect(() => {
    subscribe((next) => {
      setSeed(next);
      setRanked(null);
      setSelectedId(null);
      setShowAll(false);
    });
  }, [subscribe]);

  const baseRows = seed?.results.results ?? [];
  const byId = useMemo(
    () => new Map(baseRows.map((row) => [row.id, row])),
    [baseRows],
  );

  function toggleFeedback(id: string, label: FeedbackLabel) {
    setFeedback((current) => {
      const next = { ...current };
      if (next[id] === label) {
        delete next[id];
      } else {
        next[id] = label;
      }
      // Surface the running preference to Claude's session immediately.
      onPreference?.(
        Object.entries(next).map(([itemId, itemLabel]) => ({
          id: itemId,
          label: itemLabel,
        })),
      );
      return next;
    });
  }

  async function rerank() {
    const items: FeedbackItem[] = Object.entries(feedback).map(
      ([id, label]) => ({ id, label }),
    );
    if (items.length === 0) return;
    setRankLoading(true);
    setRankError(null);
    try {
      setRanked(
        await rankJobs({
          ids: baseRows.map((row) => row.id),
          feedback: items,
          top: baseRows.length,
        }),
      );
    } catch (cause: unknown) {
      setRankError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setRankLoading(false);
    }
  }

  // Ranked order takes precedence; otherwise the search order (+ any semantic scores).
  const rows = ranked
    ? ranked.results
        .map((entry) => byId.get(entry.id))
        .filter((row): row is NonNullable<typeof row> => row !== undefined)
    : baseRows;
  const scores = ranked
    ? Object.fromEntries(ranked.results.map((entry) => [entry.id, entry]))
    : seed?.scores;
  const feedbackCount = Object.keys(feedback).length;
  // Show a small batch by default so the user can triage at a glance.
  const visible = showAll ? rows : rows.slice(0, BATCH);

  // Detail view: rendered inline (not a fixed overlay) so it scrolls inside the
  // host-controlled iframe; "Back" returns to the list.
  if (selectedId !== null) {
    return (
      <Box p="sm">
        <Anchor
          component="button"
          type="button"
          size="sm"
          mb="xs"
          onClick={() => setSelectedId(null)}
        >
          ← Back to results
        </Anchor>
        <JobDetail jobId={selectedId} showTitle />
      </Box>
    );
  }

  return (
    <Box p="sm">
      <Group justify="space-between" mb="xs">
        <Title order={4}>joblode</Title>
        {seed ? (
          <Text c="dimmed" size="sm">
            {seed.results.total.toLocaleString()} matches
          </Text>
        ) : null}
      </Group>

      {rankError ? (
        <Alert color="red" title="Ranking failed" mb="xs">
          {rankError}
        </Alert>
      ) : null}

      {rows.length > 0 ? (
        <>
          <ResultsTable
            rows={visible}
            scores={scores}
            feedback={feedback}
            onFeedback={toggleFeedback}
            onSelect={setSelectedId}
          />
          <Group mt="sm" gap="sm">
            <Button
              size="xs"
              onClick={rerank}
              loading={rankLoading}
              disabled={feedbackCount === 0}
            >
              Re-rank by my picks ({feedbackCount})
            </Button>
            {ranked ? (
              <Button size="xs" variant="subtle" onClick={() => setRanked(null)}>
                Clear ranking
              </Button>
            ) : null}
            {!showAll && rows.length > BATCH ? (
              <Button
                size="xs"
                variant="subtle"
                onClick={() => setShowAll(true)}
              >
                Show all {rows.length}
              </Button>
            ) : null}
          </Group>
        </>
      ) : (
        <Text c="dimmed" size="sm">
          Run a search in the chat to see roles here.
        </Text>
      )}
    </Box>
  );
}
