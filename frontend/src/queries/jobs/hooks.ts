import { useEffect, useMemo, useSyncExternalStore } from 'react';
import type { EntryKind } from '../../api';
import {
  backgroundJob,
  foregroundJob,
  getJobs,
  type Job,
  subscribe,
} from './store';

/** Every tracked job, oldest first. */
export function useJobs(): Job[] {
  return useSyncExternalStore(subscribe, getJobs);
}

/** The jobs acting on one entry — e.g. a server card's busy indicator. */
export function useEntryJobs(kind: EntryKind, id: string): Job[] {
  const all = useJobs();
  return useMemo(
    () =>
      all.filter((job) => job.subject?.kind === kind && job.subject.id === id),
    [all, kind, id],
  );
}

/**
 * The running job holding an entry's claim, for a surface that marks it busy
 * and disables what the daemon would refuse. A launch is left to the launch
 * control. Selecting the job keeps every other card from re-rendering on its
 * progress.
 */
export function useBusyJob(kind: EntryKind, id: string): Job | null {
  return useSyncExternalStore(
    subscribe,
    () =>
      getJobs().find(
        (job) =>
          job.status === 'running' &&
          job.channel !== 'instance.launch' &&
          job.subject?.kind === kind &&
          job.subject.id === id,
      ) ?? null,
  );
}

const isRunningLaunch = (job: Job): boolean =>
  job.status === 'running' && job.channel === 'instance.launch';

/**
 * The running launch of an instance, for the control that becomes its progress
 * readout. Selecting the job re-renders only when its own progress moves.
 */
export function useLaunchJob(instanceId: string): Job | null {
  return useSyncExternalStore(
    subscribe,
    () =>
      getJobs().find(
        (job) => isRunningLaunch(job) && job.subject?.id === instanceId,
      ) ?? null,
  );
}

/**
 * Whether a launch of an instance is running; with a `target`, only a launch
 * joining that world or server, so one row spins rather than all of them.
 * The snapshot is the set of launches as a string, so progress ticks do not
 * re-render the caller.
 */
export function useIsLaunching(): (id: string, target?: string) => boolean {
  const key = useSyncExternalStore(subscribe, () =>
    JSON.stringify(
      getJobs()
        .filter(isRunningLaunch)
        .map((job) => [job.subject?.id, job.target]),
    ),
  );
  return useMemo(() => {
    const launching = JSON.parse(key) as [string, string | null][];
    return (id, target) =>
      launching.some(
        ([launchId, joined]) =>
          launchId === id && (target === undefined || joined === target),
      );
  }, [key]);
}

/**
 * Claim a job's display while a surface shows its progress, so it is never on
 * screen twice. The release on cleanup is what hands a dismissed dialog's job
 * back to the status bar.
 */
export function useJobDisplay(
  job: Job | null,
  shown: boolean,
  { ownsError = false }: { ownsError?: boolean } = {},
): void {
  const id = shown && job?.status === 'running' ? job.id : null;
  useEffect(() => {
    if (!id) return;
    foregroundJob(id, ownsError);
    return () => backgroundJob(id);
  }, [id, ownsError]);
}
