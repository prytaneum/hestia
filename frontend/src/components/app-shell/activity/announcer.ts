import { useEffect, useRef, useState } from 'react';

import type { ProvisionProgress } from '@/api';
import { phaseLabel } from '@/features/shared/entry/components';
import { m } from '@/paraglide/messages.js';
import type { Job } from '@/queries/jobs';
import type { JobView } from './views';

function stageOf(job: Job): string {
  const progress = job.progress as ProvisionProgress | null;
  return progress ? phaseLabel(progress.phase) : '';
}

function announcement(
  job: Job,
  title: string,
  before: string | undefined,
): string | null {
  switch (job.status) {
    case 'running': {
      const stage = stageOf(job);
      if (before === undefined)
        return m['app.jobs.panel.announce_started']({ title });
      return stage
        ? m['app.jobs.panel.announce_stage']({ title, stage })
        : null;
    }
    case 'done':
      return m['app.jobs.panel.announce_done']({ title });
    case 'failed':
      return m['app.jobs.failed_title']({ title });
    case 'cancelled':
      return m['app.jobs.panel.announce_cancelled']({ title });
    case 'interrupted':
      return m['app.jobs.panel.announce_interrupted']({ title });
  }
}

/**
 * The text for the polite status region: said when a job starts, changes
 * stage or settles, never on a percentage tick. What was already there when
 * the shell mounted is recorded silently.
 */
export function useAnnouncement(
  jobs: Job[],
  viewOf: (job: Job) => JobView,
): string {
  const [message, setMessage] = useState('');
  const known = useRef<Map<string, string> | null>(null);

  useEffect(() => {
    const previous = known.current;
    const next = new Map<string, string>();
    const said: string[] = [];
    for (const job of jobs) {
      const state =
        job.status === 'running' ? `running:${stageOf(job)}` : job.status;
      next.set(job.id, state);
      const before = previous?.get(job.id);
      if (previous && before !== state) {
        const text = announcement(job, viewOf(job).title, before);
        if (text) said.push(text);
      }
    }
    known.current = next;
    if (said.length > 0) setMessage(said.join('. '));
  }, [jobs, viewOf]);

  return message;
}
