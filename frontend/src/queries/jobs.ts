/**
 * The global job store and the job-backed mutation factory. Every
 * long-running daemon operation (server create/update, instance launch,
 * backups, content installs, java installs, downloads) routes through
 * `startJob`, so an activity surface can render every in-flight job with
 * live progress no matter which component fired it — and the progress
 * outlives that component. `useJobMutation` adds the local view: the same
 * mutation result plus the `job`/`progress` of the run it started.
 */
import {
  type QueryKey,
  type UseMutationOptions,
  type UseMutationResult,
  useMutation,
} from '@tanstack/react-query';
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import { logger } from '@/lib/log';
import type { EntryKind, JobRun, ProvisionProgress } from '../api';
import {
  HestiaError,
  JobCancelled,
  job as jobApi,
  jobId,
  TRANSPORT,
} from '../api';
import { invalidate } from './client';

const log = logger('jobs');

/** The entry a job acts on, when it acts on one. */
export interface JobEntry {
  kind: EntryKind;
  id: string;
}

export interface JobMeta {
  /** The operation, e.g. `server.create` — what an activity surface groups by. */
  kind: string;
  /** A short human fallback; the UI may localize from `kind`/`entry` instead. */
  label: string;
  entry?: JobEntry;
}

export type JobStatus = 'running' | 'done' | 'cancelled' | 'error';

export interface Job<TProgress = unknown> extends JobMeta {
  /** The daemon's job id — what its events carry and `job.cancel` takes. */
  id: string;
  status: JobStatus;
  progress: TProgress | null;
  error: HestiaError | null;
  startedAt: number;
  settledAt: number | null;
  /** Shown in the status bar; a modal foregrounds its job to hide it there. */
  background: boolean;
}

const MAX_SETTLED = 50;

const jobs = new Map<string, Job>();
const listeners = new Set<() => void>();
let snapshot: Job[] = [];
let frame: number | null = null;

function emit(): void {
  if (frame !== null) {
    cancelAnimationFrame(frame);
    frame = null;
  }
  snapshot = [...jobs.values()];
  for (const listener of listeners) listener();
}

/**
 * Coalesce a burst to one paint. The store is read through
 * `useSyncExternalStore`, so every emit re-renders each subscriber
 * synchronously — and the daemon reports progress per file, which through an
 * assets or libraries download is faster than React can commit. React reads
 * that as a runaway update loop and throws ("Maximum update depth exceeded"),
 * killing the UI mid-launch. A gauge cannot show more than a frame's worth
 * anyway, so progress paints once per frame; every other change emits at once.
 */
function emitNextFrame(): void {
  if (frame !== null) return;
  frame = requestAnimationFrame(() => {
    frame = null;
    emit();
  });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function patch(id: string, changes: Partial<Job>, coalesce = false): void {
  const job = jobs.get(id);
  if (!job) return;
  jobs.set(id, { ...job, ...changes });
  if (coalesce) emitNextFrame();
  else emit();
}

function pruneSettled(): void {
  const settled = [...jobs.values()].filter((job) => job.status !== 'running');
  for (const job of settled.slice(
    0,
    Math.max(0, settled.length - MAX_SETTLED),
  )) {
    jobs.delete(job.id);
  }
}

function asHestiaError(error: unknown): HestiaError {
  return error instanceof HestiaError
    ? error
    : new HestiaError(TRANSPORT, String(error));
}

export interface JobHandle<TData> {
  id: string;
  result: Promise<TData>;
}

/** Run one tracked job: register it, stream its progress, settle its state. */
export function startJob<TData, TProgress>(
  meta: JobMeta,
  run: (job: JobRun<TProgress>) => Promise<TData>,
): JobHandle<TData> {
  const id = jobId(meta.kind);
  const started = run({
    id,
    onProgress: (progress) => {
      if (jobs.get(id)?.status === 'running') patch(id, { progress }, true);
    },
  });

  log.debug({ id, op: meta.kind, entry: meta.entry?.id }, 'job started');
  jobs.set(id, {
    ...meta,
    id,
    status: 'running',
    progress: null,
    error: null,
    startedAt: Date.now(),
    settledAt: null,
    background: true,
  });
  pruneSettled();
  emit();

  const result = started.then(
    (data) => {
      log.debug({ id, op: meta.kind }, 'job done');
      patch(id, { status: 'done', settledAt: Date.now() });
      return data;
    },
    (error: unknown) => {
      if (error instanceof JobCancelled) {
        patch(id, { status: 'cancelled', settledAt: Date.now() });
        throw error;
      }
      patch(id, {
        status: 'error',
        error: asHestiaError(error),
        settledAt: Date.now(),
      });
      throw error;
    },
  );
  return { id, result };
}

/** Hide a job from the status bar while a modal owns its display. */
export function foregroundJob(id: string): void {
  if (jobs.get(id)?.background === false) return;
  patch(id, { background: false });
}

/** Reveal a job in the status bar after its modal is closed. */
export function backgroundJob(id: string): void {
  if (jobs.get(id)?.background === true) return;
  patch(id, { background: true });
}

/**
 * Claim a job's display while a surface shows its progress, so it is never on
 * screen twice. The release on cleanup is what hands a dismissed dialog's job
 * back to the status bar.
 */
export function useJobDisplay(job: Job | null, shown: boolean): void {
  const id = shown && job?.status === 'running' ? job.id : null;
  useEffect(() => {
    if (!id) return;
    foregroundJob(id);
    return () => backgroundJob(id);
  }, [id]);
}

/** Ask the daemon to stop a running job; it settles on its cancelled event. */
export async function cancelJob(id: string): Promise<void> {
  if (jobs.get(id)?.status !== 'running') return;
  await jobApi.cancel(id);
}

/** Drop one settled job from the store; a running job stays. */
export function dismissJob(id: string): void {
  const job = jobs.get(id);
  if (!job || job.status === 'running') return;
  jobs.delete(id);
  emit();
}

export function clearSettledJobs(): void {
  let changed = false;
  for (const job of jobs.values()) {
    if (job.status === 'running') continue;
    jobs.delete(job.id);
    changed = true;
  }
  if (changed) emit();
}

/** The store's current jobs, oldest first — for non-React code and tests. */
export function getJobs(): Job[] {
  return snapshot;
}

/** Every tracked job, oldest first. */
export function useJobs(): Job[] {
  return useSyncExternalStore(subscribe, () => snapshot);
}

export function useJob(id: string | null): Job | null {
  return useSyncExternalStore(subscribe, () =>
    id ? (jobs.get(id) ?? null) : null,
  );
}

/** The jobs acting on one entry — e.g. a server card's busy indicator. */
export function useEntryJobs(kind: EntryKind, id: string): Job[] {
  const all = useJobs();
  return useMemo(
    () => all.filter((job) => job.entry?.kind === kind && job.entry.id === id),
    [all, kind, id],
  );
}

export interface JobSpec<TData, TVariables, TProgress> {
  mutationKey: QueryKey;
  meta: (variables: TVariables) => JobMeta;
  run: (variables: TVariables, job: JobRun<TProgress>) => Promise<TData>;
  /** Key prefixes swept when the job settles, success or error. */
  invalidates?: (variables: TVariables) => QueryKey[];
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
  job: Pick<JobSpec<TData, TVariables, TProgress>, 'meta' | 'run'>;
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
    job: { meta: spec.meta, run: spec.run },
    onSettled: (_data, _error, variables) => {
      for (const key of spec.invalidates?.(variables) ?? []) invalidate(key);
    },
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
      const handle = startJob<TData, TProgress>(
        options.job.meta(variables),
        (job) => options.job.run(variables, job),
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
