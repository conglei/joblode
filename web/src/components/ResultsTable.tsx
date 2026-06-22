import { ActionIcon, Anchor, Badge, Group, Table, Text } from "@mantine/core";

import { formatSalary } from "../lib";
import type { FeedbackLabel, JobSummary, Ranked } from "../types";

interface ResultsTableProps {
  rows: JobSummary[];
  onSelect: (id: string) => void;
  /** Current reactions by id; when `onFeedback` is provided, a 👍/👎 column shows. */
  feedback?: Record<string, FeedbackLabel>;
  onFeedback?: (id: string, label: FeedbackLabel) => void;
  /** Rank info by id; when provided, a leading Score column shows and `why` tooltips. */
  scores?: Record<string, Ranked>;
}

/** The results table. Clicking a row opens its detail drawer; the apply link
 *  opens in a new tab. Optionally shows a Score column (ranked view) and 👍/👎
 *  reactions that feed the ranking feedback loop. */
export function ResultsTable({
  rows,
  onSelect,
  feedback,
  onFeedback,
  scores,
}: ResultsTableProps) {
  const showScores = scores !== undefined;
  const showReactions = onFeedback !== undefined;

  return (
    <Table highlightOnHover stickyHeader>
      <Table.Thead>
        <Table.Tr>
          {showScores ? <Table.Th>Score</Table.Th> : null}
          <Table.Th>Title</Table.Th>
          <Table.Th>Company</Table.Th>
          <Table.Th>Location</Table.Th>
          <Table.Th>Level</Table.Th>
          <Table.Th>Comp</Table.Th>
          {showReactions ? <Table.Th>Fit?</Table.Th> : null}
          <Table.Th>Apply</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {rows.map((row) => {
          const salary = formatSalary(row.salary_min_k, row.salary_max_k);
          const ranked = scores?.[row.id];
          const reaction = feedback?.[row.id];
          return (
            <Table.Tr
              key={row.id}
              onClick={() => onSelect(row.id)}
              tabIndex={0}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onSelect(row.id);
                }
              }}
              style={{ cursor: "pointer" }}
            >
              {showScores ? (
                <Table.Td>
                  {ranked ? (
                    <Badge variant="filled" title={ranked.why}>
                      {Math.round(ranked.score)}
                    </Badge>
                  ) : (
                    <Text c="dimmed">—</Text>
                  )}
                </Table.Td>
              ) : null}
              <Table.Td>{row.title}</Table.Td>
              <Table.Td>{row.company}</Table.Td>
              <Table.Td>{row.location}</Table.Td>
              <Table.Td>
                {row.level ? <Badge variant="light">{row.level}</Badge> : null}
              </Table.Td>
              <Table.Td>{salary || <Text c="dimmed">—</Text>}</Table.Td>
              {showReactions ? (
                <Table.Td>
                  <Group gap={4} wrap="nowrap">
                    <ActionIcon
                      aria-label={`Like ${row.title}`}
                      variant={reaction === "liked" ? "filled" : "subtle"}
                      color="teal"
                      onClick={(event) => {
                        event.stopPropagation();
                        onFeedback?.(row.id, "liked");
                      }}
                    >
                      👍
                    </ActionIcon>
                    <ActionIcon
                      aria-label={`Dislike ${row.title}`}
                      variant={reaction === "disliked" ? "filled" : "subtle"}
                      color="red"
                      onClick={(event) => {
                        event.stopPropagation();
                        onFeedback?.(row.id, "disliked");
                      }}
                    >
                      👎
                    </ActionIcon>
                  </Group>
                </Table.Td>
              ) : null}
              <Table.Td>
                <Anchor
                  href={row.url}
                  target="_blank"
                  rel="noreferrer"
                  onClick={(event) => event.stopPropagation()}
                >
                  Open
                </Anchor>
              </Table.Td>
            </Table.Tr>
          );
        })}
      </Table.Tbody>
    </Table>
  );
}
