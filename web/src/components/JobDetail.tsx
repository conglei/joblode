import { useEffect, useState } from "react";
import {
  Anchor,
  Badge,
  Group,
  Loader,
  Stack,
  Text,
  Title,
  TypographyStylesProvider,
} from "@mantine/core";
import ReactMarkdown from "react-markdown";

import { getJob } from "../api";
import { formatSalary } from "../lib";
import type { Job } from "../types";

interface JobDetailProps {
  jobId: string;
  /** Show the role title as a heading (the drawer renders its own title). */
  showTitle?: boolean;
}

/** Fetches and renders one role's full record (including `jd_markdown`) in normal
 *  document flow — so it scrolls with the page/iframe, not as a fixed overlay.
 *  Structured fields are LLM extractions, so the JD is always shown as the source
 *  of truth. Used inline in the MCP App card and inside the standalone drawer. */
export function JobDetail({ jobId, showTitle = false }: JobDetailProps) {
  const [job, setJob] = useState<Job | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setJob(null);
    setError(null);
    getJob(jobId)
      .then((result) => {
        if (active) setJob(result);
      })
      .catch((cause: unknown) => {
        if (active) setError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      active = false;
    };
  }, [jobId]);

  if (error) return <Text c="red">{error}</Text>;
  if (!job) return <Loader />;

  return (
    <Stack gap="sm">
      {showTitle ? <Title order={4}>{job.title}</Title> : null}
      <Text fw={500}>{job.company}</Text>
      <Group gap="xs">
        {job.level ? <Badge variant="light">{job.level}</Badge> : null}
        {job.function ? <Badge variant="light">{job.function}</Badge> : null}
        {job.remote_scope ? (
          <Badge variant="outline">{job.remote_scope}</Badge>
        ) : null}
      </Group>
      <Text size="sm" c="dimmed">
        {[job.location, formatSalary(job.salary_min_k, job.salary_max_k)]
          .filter(Boolean)
          .join(" · ")}
      </Text>
      <Anchor href={job.url} target="_blank" rel="noreferrer">
        Apply ↗
      </Anchor>
      <TypographyStylesProvider>
        <ReactMarkdown>{job.jd_markdown}</ReactMarkdown>
      </TypographyStylesProvider>
    </Stack>
  );
}
