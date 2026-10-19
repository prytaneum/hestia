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
