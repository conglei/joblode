import { useState } from "react";
import {
  Button,
  Group,
  SegmentedControl,
  Stack,
  Text,
  Textarea,
  Title,
} from "@mantine/core";

/** The method for the optional cheap-model pass. "free" needs no key. */
export type RankMethod = "free" | "match" | "pairwise";

interface RankPanelProps {
  /** How many roles the user has reacted to (drives the helper text). */
  feedbackCount: number;
  /** True while a rank request is in flight, or when there's nothing to rank. */
  loading: boolean;
  disabled: boolean;
  ranked: boolean;
  onRank: (options: { resume: string; method: RankMethod }) => void;
  onClear: () => void;
}

/** Sidebar controls for ranking the current results: optional resume + method,
 *  plus a hint that 👍/👎 reactions personalize the free ranking. */
export function RankPanel({
  feedbackCount,
  loading,
  disabled,
  ranked,
  onRank,
  onClear,
}: RankPanelProps) {
  const [resume, setResume] = useState("");
  const [method, setMethod] = useState<RankMethod>("free");

  return (
    <Stack gap="xs">
      <Title order={5}>Rank</Title>
      <Text size="xs" c="dimmed">
        React with 👍/👎 on results, then rank — feedback personalizes the free
        order. {feedbackCount} reacted.
      </Text>
      <SegmentedControl
        size="xs"
        value={method}
        onChange={(value) => setMethod(value as RankMethod)}
        data={[
          { label: "Free", value: "free" },
          { label: "Match", value: "match" },
          { label: "Pairwise", value: "pairwise" },
        ]}
      />
      {method !== "free" ? (
        <Textarea
          label="Resume"
          description="Required for match / pairwise (needs a configured model)."
          placeholder="Paste your resume…"
          autosize
          minRows={3}
          maxRows={8}
          value={resume}
          onChange={(event) => setResume(event.currentTarget.value)}
        />
      ) : null}
      <Group grow>
        <Button
          onClick={() => onRank({ resume, method })}
          loading={loading}
          disabled={disabled}
        >
          Rank results
        </Button>
        {ranked ? (
          <Button variant="default" onClick={onClear}>
            Clear
          </Button>
        ) : null}
      </Group>
    </Stack>
  );
}
