import { Anchor, Badge, Table, Text } from "@mantine/core";

import { formatSalary } from "../lib";
import type { JobSummary } from "../types";

interface ResultsTableProps {
  rows: JobSummary[];
  onSelect: (id: string) => void;
}

/** The streaming-friendly results table. Clicking a row opens its detail
 *  drawer; the apply link opens in a new tab without selecting the row. */
export function ResultsTable({ rows, onSelect }: ResultsTableProps) {
  return (
    <Table highlightOnHover stickyHeader>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>Title</Table.Th>
          <Table.Th>Company</Table.Th>
          <Table.Th>Location</Table.Th>
          <Table.Th>Level</Table.Th>
          <Table.Th>Comp</Table.Th>
          <Table.Th>Apply</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {rows.map((row) => {
          const salary = formatSalary(row.salary_min_k, row.salary_max_k);
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
              <Table.Td>{row.title}</Table.Td>
              <Table.Td>{row.company}</Table.Td>
              <Table.Td>{row.location}</Table.Td>
              <Table.Td>
                {row.level ? <Badge variant="light">{row.level}</Badge> : null}
              </Table.Td>
              <Table.Td>{salary || <Text c="dimmed">—</Text>}</Table.Td>
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
