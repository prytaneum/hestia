import { PlayIcon, XIcon } from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';

import type { ProvisionProgress, QuickPlay } from '@/api';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { useLaunch } from '@/features/instances/hooks';
import {
  LaunchGauge,
  type LaunchReading,
} from '@/features/instances/lib/launch-gauge';
import { phaseLabel } from '@/features/shared/entry/components/provision-progress';
import { bytesOf, bytesPerSecond } from '@/lib/format';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import {
  cancelJob,
  type Job,
  useJobDisplay,
  useLaunchJob,
} from '@/queries/jobs';

/** A launch settling sooner than this never shows progress. */
export const SHOW_AFTER_MS = 300;
/** Once shown, progress stays at least this long. */
const MIN_SHOWN_MS = 400;

type Size = 'sm' | 'md' | 'bar';

const BOX: Record<Size, string> = {
  sm: 'h-7 w-44',
  md: 'h-8 w-56',
  bar: 'h-10 w-72',
};

const IDLE: Record<Size, { size: 'sm' | 'default'; height?: string }> = {
  sm: { size: 'sm' },
  md: { size: 'default' },
  bar: { size: 'default', height: 'h-10' },
};

/**
 * Whether the progress state is on screen: it waits `SHOW_AFTER_MS` for a
 * launch to still be running, and once up it outlasts the launch to
 * `MIN_SHOWN_MS`, so a quick launch never flashes.
 */
function useProgressShown(running: boolean): boolean {
  const [shown, setShown] = useState(false);
  const shownAt = useRef(0);
  useEffect(() => {
    if (running === shown) return;
    const wait = running
      ? SHOW_AFTER_MS
      : Math.max(0, MIN_SHOWN_MS - (Date.now() - shownAt.current));
    const timer = setTimeout(() => {
      shownAt.current = Date.now();
      setShown(running);
    }, wait);
    return () => clearTimeout(timer);
  }, [running, shown]);
  return shown;
}

function etaLabel(seconds: number): string {
  if (seconds < 60) return m['instance.launch.eta_seconds']();
  const minutes = Math.round(seconds / 60);
  return minutes < 90
    ? m['instance.launch.eta_minutes']({ count: minutes })
    : m['instance.launch.eta_hours']({ count: Math.round(minutes / 60) });
}

function detailOf(reading: LaunchReading | null): string | null {
  if (!reading || reading.total <= 0) return null;
  return [
    bytesOf(reading.current, reading.total),
    reading.rate > 0 && bytesPerSecond(reading.rate),
    reading.etaSeconds !== null && etaLabel(reading.etaSeconds),
  ]
    .filter(Boolean)
    .join(' · ');
}

/**
 * The Play (or Join) button, which becomes the launch's progress readout while
 * the instance launches: ember fill, stage and percent on its face, Cancel
 * always beside it. The state comes from the job store, so a launch started
 * anywhere (another surface, the CLI, before a reload) reads the same. A
 * Quick Play control shows progress only for its own target; the other
 * controls of that instance just disable.
 *
 * Callers swap in the run control once a session is up.
 */
export function LaunchControl({
  instanceId,
  size = 'md',
  label = 'play',
  quickPlay,
  newSession,
  disabled = false,
  disabledReason,
}: {
  instanceId: string;
  size?: Size;
  label?: 'play' | 'join';
  quickPlay?: QuickPlay;
  newSession?: boolean;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const { launch } = useLaunch();
  const launching = useLaunchJob(instanceId);
  const own =
    launching && (launching.target ?? null) === (quickPlay?.target ?? null)
      ? launching
      : null;
  const shown = useProgressShown(own !== null);
  const last = useRef<Job | null>(null);
  if (own) last.current = own;
  useJobDisplay(own, shown);

  const action =
    label === 'join' ? m['app.action.join']() : m['app.action.play']();
  const job = shown ? (own ?? last.current) : null;
  if (job)
    return (
      <LaunchProgress key={job.id} job={job} size={size} action={action} />
    );

  return (
    <Button
      size={IDLE[size].size}
      data-icon="inline-start"
      disabled={disabled || launching !== null}
      title={disabled ? disabledReason : undefined}
      className={cn(
        'bg-ember text-ember-foreground hover:bg-ember/90',
        IDLE[size].height,
      )}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        launch(instanceId, { quickPlay, newSession });
      }}
    >
      {own ? <Spinner aria-hidden /> : <PlayIcon weight="fill" />}
      {action}
    </Button>
  );
}

function LaunchProgress({
  job,
  size,
  action,
}: {
  job: Job;
  size: Size;
  action: string;
}) {
  const progress = job.progress as ProvisionProgress | null;
  const [gauge] = useState(() => new LaunchGauge());
  const [reading, setReading] = useState<LaunchReading | null>(null);
  useEffect(() => {
    if (progress) setReading(gauge.observe(progress));
  }, [gauge, progress]);

  const stage = reading?.starting
    ? m['instance.launch.starting']()
    : phaseLabel(reading?.phase ?? 'resolving');
  const fill = reading?.fill ?? null;
  const percent =
    fill !== null && !reading?.starting ? Math.floor(fill * 100) : null;
  const status =
    percent === null ? stage : m['instance.launch.status']({ stage, percent });
  const detail = detailOf(reading);

  const face = (
    <span className="flex min-w-0 flex-col justify-center px-2.5 leading-tight">
      <span className="flex items-baseline gap-1">
        <span className="truncate">{stage}</span>
        {percent !== null && (
          <span className="shrink-0 tabular-nums">{percent}%</span>
        )}
      </span>
      {size === 'bar' && detail && (
        <span className="truncate text-[11px] font-normal tabular-nums opacity-80">
          {detail}
        </span>
      )}
    </span>
  );

  return (
    <div
      className={cn(
        'flex items-stretch border border-border bg-muted',
        BOX[size],
      )}
    >
      <div
        role="progressbar"
        aria-label={m['instance.launch.name']({
          action,
          status: status.toLocaleLowerCase(),
        })}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
        aria-valuetext={
          percent === null
            ? stage
            : m['instance.launch.value_text']({ stage, percent })
        }
        aria-busy={percent === null || undefined}
        title={detail ?? undefined}
        className="relative flex min-w-0 flex-1 flex-col justify-center overflow-hidden text-xs font-medium text-foreground"
      >
        {face}
        {fill !== null ? (
          <div
            aria-hidden
            className="absolute inset-0 flex flex-col justify-center bg-ember text-ember-foreground transition-[clip-path] duration-300 ease-linear motion-reduce:transition-none"
            style={{ clipPath: `inset(0 ${100 - fill * 100}% 0 0)` }}
          >
            {face}
          </div>
        ) : (
          <div
            aria-hidden
            className="absolute inset-x-0 bottom-0 h-0.5 overflow-hidden bg-ember/30 motion-reduce:bg-ember/60"
          >
            <div className="progress-sweep h-full bg-ember motion-reduce:hidden" />
          </div>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={m['instance.launch.cancel']()}
        title={m['instance.launch.cancel']()}
        className="h-full w-7 shrink-0 border-l border-border"
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          cancelJob(job.id).catch(() => {});
        }}
      >
        <XIcon />
      </Button>
    </div>
  );
}
