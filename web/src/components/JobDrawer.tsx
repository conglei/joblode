import { Drawer } from "@mantine/core";

import { JobDetail } from "./JobDetail";

interface JobDrawerProps {
  jobId: string | null;
  onClose: () => void;
}

/** The standalone web app's detail surface: a right-side drawer over the full-height
 *  page. (The embedded MCP App card renders {@link JobDetail} inline instead, since a
 *  fixed overlay can't scroll inside a host-scrolled iframe.) */
export function JobDrawer({ jobId, onClose }: JobDrawerProps) {
  return (
    <Drawer
      opened={jobId !== null}
      onClose={onClose}
      position="right"
      size="lg"
      title="Role details"
    >
      {jobId !== null ? <JobDetail jobId={jobId} showTitle /> : null}
    </Drawer>
  );
}
