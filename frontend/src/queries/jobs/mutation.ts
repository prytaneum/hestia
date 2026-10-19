import {
  type QueryKey,
  type UseMutationOptions,
  type UseMutationResult,
  useMutation,
} from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import type { HestiaError, JobRun, ProvisionProgress } from '../../api';
import { useJobs } from './hooks';
import { type Job, type JobMeta, startJob } from './store';

export interface JobSpec<TData, TVariables, TProgress> {
  mutationKey: QueryKey;
  meta: (variables: TVariables) => JobMeta;
  run: (variables: TVariables, job: JobRun) => Promise<TData>;
  /** Key prefixes swept when the job settles, however it ends. */
  invalidates?: (variables: TVariables) => QueryKey[];
  /** Type-only: carries `TProgress` to the hook's result; never set. */
  progress?: TProgress;
}

/**
 * `UseMutationOptions` carrying the job descriptor but **no** `mutationFn`:
 * `useJobMutation` is the single owner of the `startJob` call, so there is
 * exactly one path that starts a job. These options are not meant for plain
 * `useMutation` — that would have no `mutationFn` to run.
 */
export type JobMutationOptions<TData, TVariables, TProgress> = Omit<
  UseMutationOptions<TData, HestiaError, TVariables>,
  'mutationFn'
> & {
  job: Pick<
    JobSpec<TData, TVariables, TProgress>,
    'meta' | 'run' | 'invalidates'
  >;
};

export function jobMutation<
  TData,
  TVariables = void,
  TProgress = ProvisionProgress,
>(
  spec: JobSpec<TData, TVariables, TProgress>,
): JobMutationOptions<TData, TVariables, TProgress> {
  return {
    mutationKey: spec.mutationKey,
    job: { meta: spec.meta, run: spec.run, invalidates: spec.invalidates },
  };
}

export type JobMutationResult<TData, TVariables, TProgress> = UseMutationResult<
  TData,
  HestiaError,
  TVariables
> & {
  /** The store's view of the run this hook instance started last. */
  job: Job<TProgress> | null;
  progress: TProgress | null;
};

/**
 * `useMutation` plus live progress for the job(s) this call site starts.
 *
 * A hook instance can fire more than once; the exposed `job` follows the
 * newest run that is still running (falling back to the most recent), so a
 * second trigger never freezes the first run's progress while it is live.
 * Every run is tracked in the global store regardless, so concurrent jobs are
 * never lost — a fan-out surface reads them through `useEntryJobs`.
 */
export function useJobMutation<TData, TVariables, TProgress>(
  options: JobMutationOptions<TData, TVariables, TProgress>,
): JobMutationResult<TData, TVariables, TProgress> {
  const [startedIds, setStartedIds] = useState<string[]>([]);
  const mutation = useMutation<TData, HestiaError, TVariables>({
    ...options,
    mutationFn: (variables) => {
      const handle = startJob<TData>(
        options.job.meta(variables),
        (job) => options.job.run(variables, job),
        options.job.invalidates?.(variables),
      );
      setStartedIds((ids) => [...ids, handle.id]);
      return handle.result;
    },
  });
  const all = useJobs();
  const job = useMemo(() => {
    const mine = startedIds
      .map((id) => all.find((entry) => entry.id === id))
      .filter((entry): entry is Job => entry !== undefined);
    const running = mine.filter((entry) => entry.status === 'running');
    return (
      ((running.at(-1) ?? mine.at(-1)) as Job<TProgress> | undefined) ?? null
    );
  }, [all, startedIds]);
  return { ...mutation, job, progress: job?.progress ?? null };
}
