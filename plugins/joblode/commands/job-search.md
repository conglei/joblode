---
description: Start an agent-driven job hunt over the joblode dataset.
---

Start a job search with the joblode tools.

If the user gave criteria in `$ARGUMENTS`, use them as the starting filters;
otherwise ask what they're looking for (function, level, location, comp floor, and a
one-line description of the work).

Then run the joblode **job-search** workflow: narrow → search (filters + optional query) →
present roles for 👍/👎 validation → `rank_jobs` with the accumulated feedback →
`get_job` for the top few (confirm comp/auth/location against `jd_markdown`) → track
the shortlist and the user's taste for next time.

$ARGUMENTS
