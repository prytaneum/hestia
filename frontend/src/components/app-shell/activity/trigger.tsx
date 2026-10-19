import { CaretUpIcon, ClockCounterClockwiseIcon } from '@phosphor-icons/react';

import type { ProvisionProgress } from '@/api';
import { PopoverTrigger } from '@/components/ui/popover';
import { Spinner } from '@/components/ui/spinner';
import {
  isMeasurable,
  overallRatio,
  phaseLabel,
} from '@/features/shared/entry/components';
import { m } from '@/paraglide/messages.js';
import type { Job } from '@/queries/jobs';
import type { JobView } from './views';

const TRIGGER =
  'flex h-6 min-w-0 items-center gap-2 px-1.5 text-[11px] text-muted-foreground outline-none hover:text-foreground focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring aria-expanded:text-foreground';

function summaryOf(running: number, failed: number): string {
  return [
    running > 0 && m['app.jobs.panel.running_count']({ count: running }),
    failed > 0 && m['app.jobs.panel.failed_count']({ count: failed }),
  ]
    .filter(Boolean)
    .join(', ');
}

export function ActivityTrigger({
  running,
  unseenFailures,
  viewOf,
}: {
  running: Job[];
  unseenFailures: number;
  viewOf: (job: Job) => JobView;
}) {
  const summary = summaryOf(running.length, unseenFailures);
  const label = summary
    ? m['app.jobs.panel.summary']({ summary })
    : m['app.jobs.panel.label']();
  const primary = running[0];

  if (primary) {
    const progress = primary.progress as ProvisionProgress | null;
    const percent =
      progress && isMeasurable(progress)
        ? Math.round(overallRatio(progress) * 100)
        : null;
    const stage = progress ? phaseLabel(progress.phase) : null;
    return (
      <PopoverTrigger className={`${TRIGGER} max-w-full`} aria-label={label}>
        <Spinner className="size-3.5 shrink-0 text-ember" />
        <span className="truncate">
          {[viewOf(primary).title, stage].filter(Boolean).join(' · ')}
        </span>
        {percent !== null && (
          <span className="shrink-0 tabular-nums">· {percent}%</span>
        )}
        {running.length > 1 && (
          <span className="shrink-0">
            {m['app.jobs.more']({ count: running.length - 1 })}
          </span>
        )}
        <CaretUpIcon className="size-3 shrink-0" />
      </PopoverTrigger>
    );
  }

  if (unseenFailures > 0) {
    return (
      <PopoverTrigger className={TRIGGER} aria-label={label}>
        <span className="size-2 shrink-0 bg-destructive" />
        {m['app.jobs.panel.failed_count']({ count: unseenFailures })}
        <CaretUpIcon className="size-3 shrink-0" />
      </PopoverTrigger>
    );
  }

  return (
    <PopoverTrigger
      className={TRIGGER}
      aria-label={label}
      title={m['app.jobs.panel.label']()}
    >
      <ClockCounterClockwiseIcon className="size-3.5" />
    </PopoverTrigger>
  );
}
