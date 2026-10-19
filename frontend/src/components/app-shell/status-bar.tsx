import { CaretUpIcon, XIcon } from '@phosphor-icons/react';
import { Link } from '@tanstack/react-router';

import type { ProvisionProgress } from '@/api';
import { Button } from '@/components/ui/button';
import {
  Popover,
  PopoverContent,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/ui/popover';
import { Spinner } from '@/components/ui/spinner';
import { StatusDot } from '@/components/ui/status-dot';
import { anchor } from '@/features/onboarding';
import {
  isMeasurable,
  overallRatio,
  ProvisionProgressView,
} from '@/features/shared/entry/components';
import { m } from '@/paraglide/messages.js';
import { useDaemon } from '@/queries/daemon';
import { useInstances } from '@/queries/instance';
import { cancelJob, type Job, jobTitle, useJobs } from '@/queries/jobs';
import { useNetwork } from '@/queries/net';
import { useServers } from '@/queries/server';

export function StatusBar() {
  const daemon = useDaemon();
  const jobs = useJobs();
  const instances = useInstances();
  const servers = useServers();

  const running = jobs.filter(
    (job) => job.status === 'running' && job.background,
  );
  const nameOf = (job: Job): string => {
    const entries =
      job.subject?.kind === 'instance'
        ? instances.data
        : job.subject?.kind === 'server'
          ? servers.data
          : undefined;
    return jobTitle(job, entries?.find((e) => e.id === job.subject?.id)?.name);
  };

  return (
    <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-border bg-sidebar px-4 text-[11px] text-muted-foreground">
      <span
        className="inline-flex items-center gap-1.5"
        {...anchor('daemon-status')}
      >
        <StatusDot tone={daemon.connected ? 'on' : 'off'} />
        {daemon.connected
          ? m['app.daemon.connected']()
          : m['app.daemon.offline']()}
      </span>
      {daemon.connected && <NetworkIndicator />}
      {daemon.status && (
        <span className="font-mono">v{daemon.status.version}</span>
      )}

      {running.length > 0 && <JobActivity jobs={running} nameOf={nameOf} />}
    </footer>
  );
}

/**
 * Reachability beside the daemon light, and deliberately separate from it: the
 * daemon can be perfectly healthy with no way out to the internet, and the two
 * failures need different things from the user.
 */
function NetworkIndicator() {
  const network = useNetwork();
  const state = network?.state ?? 'unknown';
  const label =
    state === 'online'
      ? m['app.network.online']()
      : state === 'unknown'
        ? m['app.network.checking']()
        : network?.offlineMode
          ? m['app.network.pinned']()
          : m['app.network.offline']();

  const title =
    state === 'offline'
      ? network?.offlineMode
        ? m['app.network.pinned_label']()
        : m['app.network.offline_label']()
      : m['app.network.label']();

  const dot = (
    <StatusDot
      tone={state === 'online' ? 'on' : state === 'unknown' ? 'off' : 'warn'}
    />
  );

  // Offline is the one state with somewhere to go: the page explains what still
  // works. Online has nothing to say, so it stays inert text.
  if (state !== 'offline') {
    return (
      <span className="inline-flex items-center gap-1.5" title={title}>
        {dot}
        {label}
      </span>
    );
  }

  return (
    <Link
      to="/offline"
      className="inline-flex items-center gap-1.5 hover:text-foreground"
      title={title}
    >
      {dot}
      {label}
    </Link>
  );
}

/**
 * A compact, non-blocking readout of the background jobs in flight — the first
 * job inline, and a popover listing every one with its live progress. It never
 * blocks the app; the jobs run whichever page the user is on.
 */
function JobActivity({
  jobs,
  nameOf,
}: {
  jobs: Job[];
  nameOf: (job: Job) => string;
}) {
  const primary = jobs[0];
  const progress = primary.progress as ProvisionProgress | null;
  const pct =
    progress && isMeasurable(progress)
      ? Math.round(overallRatio(progress) * 100)
      : null;

  return (
    <Popover>
      <PopoverTrigger className="ml-auto flex min-w-0 max-w-[55%] items-center gap-2 text-[11px] text-muted-foreground outline-none hover:text-foreground focus-visible:text-foreground">
        <Spinner className="size-3.5 shrink-0 text-ember" />
        <span className="truncate">{nameOf(primary)}</span>
        <div className="relative h-1 w-16 shrink-0 overflow-hidden bg-muted">
          {pct === null ? (
            <div className="progress-sweep absolute inset-y-0 left-0 bg-ember" />
          ) : (
            <div
              className="h-full bg-ember transition-all"
              style={{ width: `${pct}%` }}
            />
          )}
        </div>
        {jobs.length > 1 && (
          <span className="shrink-0">
            {m['app.jobs.more']({ count: jobs.length - 1 })}
          </span>
        )}
        <CaretUpIcon className="size-3 shrink-0" />
      </PopoverTrigger>
      <PopoverContent side="top" align="end" className="w-80 gap-0 p-0">
        <PopoverTitle className="border-b border-border p-2.5 text-xs font-medium">
          {m['app.jobs.title']({ count: jobs.length })}
        </PopoverTitle>
        <div className="max-h-72 divide-y divide-border overflow-y-auto p-1">
          {jobs.map((job) => (
            <div key={job.id} className="flex flex-col gap-1.5 p-2">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-xs font-medium text-foreground">
                  {nameOf(job)}
                </span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={m['app.action.cancel']()}
                  onClick={() => {
                    cancelJob(job.id).catch(() => {});
                  }}
                >
                  <XIcon />
                </Button>
              </div>
              <ProvisionProgressView
                progress={job.progress as ProvisionProgress | null}
              />
            </div>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}
