import { useEffect, useState } from "react";
import {
  Anchor,
  Badge,
  Drawer,
  Group,
  Loader,
  Stack,
  Text,
  TypographyStylesProvider,
} from "@mantine/core";
import ReactMarkdown from "react-markdown";

import { getJob } from "../api";
import { formatSalary } from "../lib";
import type { Job } from "../types";

interface JobDrawerProps {
  jobId: string | null;
  onClose: () => void;
}

/** Lazily fetches and renders one role's full record (including `jd_markdown`)
 *  whenever `jobId` changes. Structured fields are LLM extractions — the JD is
 *  the source of truth, so we always show it. */
export function JobDrawer({ jobId, onClose }: JobDrawerProps) {
  const [job, setJob] = useState<Job | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (jobId === null) return;
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

  return (
    <Drawer
      opened={jobId !== null}
      onClose={onClose}
      position="right"
      size="lg"
      title={job ? job.title : "Role"}
    >
      {error ? <Text c="red">{error}</Text> : null}
      {!job && !error ? <Loader /> : null}
      {job ? (
        <Stack gap="sm">
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
      ) : null}
    </Drawer>
  );
}
