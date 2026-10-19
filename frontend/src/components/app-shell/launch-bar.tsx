import { CheckIcon, PlayIcon, WarningIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { errorMessage } from '@/api';
import { Button } from '@/components/ui/button';
import { Thumbnail } from '@/components/ui/thumbnail';
import {
  LaunchControl,
  SHOW_AFTER_MS,
} from '@/features/instances/components/launch-control';
import { setNotificationsOpen } from '@/lib/notifications';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useInstances } from '@/queries/instance';
import { getJobs, type Job, useRunningLaunchIds } from '@/queries/jobs';

const HANDOFF_MS = 1000;

export interface LaunchBarState {
  job: Job;
  more: number;
}

/**
 * The launch the play bar carries, if any: the newest running one, shown only
 * once it outlasts `SHOW_AFTER_MS`, and kept for `HANDOFF_MS` after it settles
 * so its outcome is seen. A cancelled launch leaves at once. Progress ticks do
 * not re-render the caller.
 */
export function useLaunchBar(): LaunchBarState | null {
  const ids = useRunningLaunchIds();
  const newest = ids.at(-1) ?? null;
  const [pinned, setPinned] = useState<string | null>(null);

  useEffect(() => {
    if (newest) {
      const timer = setTimeout(
        () => setPinned(newest),
        pinned ? 0 : SHOW_AFTER_MS,
      );
      return () => clearTimeout(timer);
    }
    if (!pinned) return;
    const status = getJobs().find((job) => job.id === pinned)?.status;
    const settledWithOutcome = status === 'done' || status === 'failed';
    const timer = setTimeout(
      () => setPinned(null),
      settledWithOutcome ? HANDOFF_MS : 0,
    );
    return () => clearTimeout(timer);
  }, [newest, pinned]);

  const job = getJobs().find((candidate) => candidate.id === pinned);
  if (!job || job.status === 'cancelled' || job.status === 'interrupted')
    return null;
  return { job, more: Math.max(0, ids.length - 1) };
}

/**
 * The play bar while a launch runs: its instance (and quick-play target) pinned
 * at the left, the launch's own progress readout at the right, whatever route
 * is showing. It keeps the play bar's height so nothing below it moves.
 */
export function LaunchBar({ job, more }: LaunchBarState) {
  const instances = useInstances();
  const instance = instances.data?.find((i) => i.id === job.subject?.id);
  if (!instance) return null;

  return (
    <div className="flex h-[76px] animate-in items-center gap-3 border-t border-border bg-sidebar px-4 duration-200 fade-in-0 slide-in-from-bottom-4 motion-reduce:animate-none">
      <div className="flex h-14 w-72 items-center gap-3">
        <Thumbnail fallback={instance.iconUrl} icon={PlayIcon} size="xl" />
        <span className="min-w-0 flex-1 leading-tight">
          <span className="block truncate text-base font-medium">
            {instance.name}
          </span>
          <span className="block truncate text-xs text-muted-foreground">
            {job.target ?? instance.gameVersion}
          </span>
        </span>
      </div>

      <div className="ml-auto flex items-center gap-3">
        {more > 0 && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setNotificationsOpen(true)}
          >
            {m['app.notifications.more']({ count: more })}
          </Button>
        )}
        {job.status === 'running' ? (
          <LaunchControl
            instanceId={instance.id}
            size="bar"
            label={job.target ? 'join' : 'play'}
            quickPlay={
              job.target ? { kind: 'world', target: job.target } : undefined
            }
          />
        ) : (
          <Handoff job={job} />
        )}
      </div>
    </div>
  );
}

function Handoff({ job }: { job: Job }) {
  const failed = job.status === 'failed';
  const Glyph = failed ? WarningIcon : CheckIcon;
  const line =
    failed && job.error ? errorMessage(job.error) : m['app.playbar.started']();
  return (
    <div
      title={line}
      className={cn(
        'flex h-10 w-72 items-center gap-2 border border-border bg-muted px-2.5 text-xs font-medium',
        failed ? 'text-destructive' : 'text-foreground',
      )}
    >
      <Glyph className="size-4 shrink-0" />
      <span className="truncate">{line}</span>
    </div>
  );
}
