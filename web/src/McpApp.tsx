import { useEffect, useState } from "react";
import { Box, Group, Text, Title } from "@mantine/core";

import { JobDrawer } from "./components/JobDrawer";
import { ResultsTable } from "./components/ResultsTable";
import type { Seed } from "./lib";

interface McpAppProps {
  /** Seed captured before mount (the tool result that opened the app), if any. */
  initial: Seed | null;
  /** Registers a callback for tool results pushed after mount (a later search). */
  subscribe: (onSeed: (seed: Seed) => void) => void;
}

/** The embedded MCP App card (DESIGN §7/§13): a compact results view rendered
 *  *from the tool result Claude pushed* (`search_jobs` / `semantic_search`). No
 *  filter sidebar — Claude drives the search; this surfaces the rows and a detail
 *  drawer. Distinct from the standalone {@link App}, which owns its own search UI. */
export function McpApp({ initial, subscribe }: McpAppProps) {
  const [seed, setSeed] = useState<Seed | null>(initial);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  // A later search Claude runs pushes a new result; reflect it live.
  useEffect(() => {
    subscribe(setSeed);
  }, [subscribe]);

  const rows = seed?.results.results ?? [];

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

      {rows.length > 0 ? (
        <ResultsTable rows={rows} scores={seed?.scores} onSelect={setSelectedId} />
      ) : (
        <Text c="dimmed" size="sm">
          Run a search in the chat to see roles here.
        </Text>
      )}

      <JobDrawer jobId={selectedId} onClose={() => setSelectedId(null)} />
    </Box>
  );
}
