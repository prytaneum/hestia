import { XIcon } from '@phosphor-icons/react';

import type { ProvisionProgress } from '@/api';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { m } from '@/paraglide/messages.js';
import { cancelJob, type Job, jobTitle, jobVerb } from '@/queries/jobs';
import {
  isMeasurable,
  overallRatio,
  ProvisionProgressView,
} from './provision-progress';

/** Why the daemon would refuse an action on an entry this job holds. */
export const busyReason = (job: Job): string =>
  m['app.jobs.busy']({ verb: jobVerb(job) });

/**
 * A running job on an entry's card or row: spinner, verb and, once measured,
 * the percentage. Text carries the state, not colour, and the chip is not a
 * live region — Activity's status region announces start, stage and settle.
 */
export function EntryJobChip({ job }: { job: Job }) {
  const progress = job.progress as ProvisionProgress | null;
  const percent =
    progress && isMeasurable(progress)
      ? Math.round(overallRatio(progress) * 100)
      : null;
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5">
      <Spinner
        role="presentation"
        aria-hidden
        aria-label={undefined}
        className="size-3 shrink-0"
      />
      <span className="truncate">{jobVerb(job)}</span>
      {percent !== null && <span className="tabular-nums">{percent}%</span>}
    </span>
  );
}

export function CancelJobButton({ job, title }: { job: Job; title: string }) {
  return (
    <Button
      variant="ghost"
      size="icon-xs"
      aria-label={m['app.jobs.panel.cancel']({ title })}
      onClick={() => {
        cancelJob(job.id).catch(() => {});
      }}
    >
      <XIcon />
    </Button>
  );
}

/** The entry hero's view of its running job: compact progress and Cancel. */
export function EntryJobProgress({ job, name }: { job: Job; name: string }) {
  return (
    <div className="flex items-center gap-3">
      <ProvisionProgressView
        compact
        className="min-w-0 flex-1"
        progress={job.progress as ProvisionProgress | null}
      />
      <CancelJobButton job={job} title={jobTitle(job, name)} />
    </div>
  );
}
