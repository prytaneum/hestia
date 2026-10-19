import { useEffect } from 'react';

import { Popover } from '@/components/ui/popover';
import {
  markFailuresSeen,
  setActivityOpen,
  useActivity,
  useJobs,
} from '@/queries/jobs';
import { useAnnouncement } from './announcer';
import { ActivityPanel } from './panel';
import { ActivityTrigger } from './trigger';
import { useJobViews } from './views';

/**
 * The status bar's window onto jobs: a trigger that summarises what runs or
 * failed, and a panel with every running job and the recent history. A job a
 * dialog has claimed stays out of the summary but is listed in the panel.
 */
export function Activity() {
  const jobs = useJobs();
  const { open, focus } = useActivity();
  const viewOf = useJobViews();

  const running = jobs.filter(
    (job) => job.status === 'running' && job.background,
  );
  const unseenFailures = jobs.filter(
    (job) => job.status === 'failed' && !job.seen,
  ).length;
  const message = useAnnouncement(
    jobs.filter((job) => job.background),
    viewOf,
  );

  useEffect(() => {
    if (open && unseenFailures > 0) markFailuresSeen();
  }, [open, unseenFailures]);

  return (
    <div className="ml-auto flex min-w-0 max-w-[55%] items-center">
      <span role="status" className="sr-only">
        {message}
      </span>
      <Popover open={open} onOpenChange={setActivityOpen}>
        <ActivityTrigger
          running={running}
          unseenFailures={unseenFailures}
          viewOf={viewOf}
        />
        <ActivityPanel jobs={jobs} viewOf={viewOf} focus={focus} />
      </Popover>
    </div>
  );
}
