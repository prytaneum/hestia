/**
 * The global job store: a mirror of the daemon's running jobs. Every
 * long-running daemon operation (server create/update, instance launch,
 * backups, content installs, java installs, downloads) lands here whichever
 * client started it, so an activity surface can render all of them with live
 * progress and the progress outlives the component that fired it.
 *
 * The daemon's `job` events are the source of truth: one listener adopts jobs
 * this webview did not start, applies progress and settles. A webview reload
 * or a daemon reconnect re-reads `job.list`.
 */
import type { QueryKey } from '@tanstack/react-query';
import { logger } from '@/lib/log';
import { notify, type Outcome as Toast } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import type { EntryKind, JobRun, Jobs, Topics } from '../../api';
import {
  HANDLER_ERROR,
  HestiaError,
  JobCancelled,
  JobInterrupted,
  job as jobApi,
  jobId,
  onConnectionChange,
  onTopic,
  TRANSPORT,
} from '../../api';
import { invalidate } from '../client';
import { entryName, jobTitle } from './title';

const log = logger('jobs');

/** The entry a job acts on, when it acts on one. */
export interface JobEntry {
  kind: EntryKind;
  id: string;
}

export interface JobMeta {
  /** The job's start channel; its title is the `app.jobs.kind.<channel>` message. */
  channel: keyof Jobs;
  subject?: JobEntry;
  /** What a launch joins (a world folder or server address); absent for a plain launch. */
  target?: string;
}

export type JobStatus =
  | 'running'
  | 'done'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

type Outcome =
  | { status: 'done'; done: unknown }
  | { status: 'failed'; error: HestiaError }
  | { status: 'cancelled' }
  | { status: 'interrupted' };

export interface Job<TProgress = unknown> {
  /** The daemon's job id — what its events carry and `job.cancel` takes. */
  id: string;
  channel: string;
  subject: JobEntry | null;
  /** The join target of an own launch; the daemon does not report it, so an adopted job has none. */
  target: string | null;
  /** `own` when this webview started it; `foreign` when it was adopted. */
  origin: 'own' | 'foreign';
  status: JobStatus;
  progress: TProgress | null;
  done: unknown;
  error: HestiaError | null;
  startedAt: number;
  settledAt: number | null;
  /** Shown in the status bar; a modal foregrounds its job to hide it there. */
  background: boolean;
  /** The claiming surface renders this job's failure itself. */
  ownsError: boolean;
}

const MAX_SETTLED = 50;

/** The outcome toast a job declares; the store adds the id it is keyed by. */
export type JobOutcome = Omit<Toast, 'id'>;

interface JobOptions<TData> {
  /** Key prefixes swept when the job settles, however it ends. */
  invalidates?: QueryKey[];
  /** Describes a finished run for the toast; `null` says nothing. */
  notify?: (done: TData) => JobOutcome | null;
  /** Starts the same run again, offered on the failure toast. */
  retry?: () => void;
}

/** What only this webview's own jobs carry: what to sweep, say, retry and abort. */
interface Own {
  sweep: QueryKey[];
  notify?: (done: unknown) => JobOutcome | null;
  retry?: () => void;
  abort: AbortController;
}

const jobs = new Map<string, Job>();
const owned = new Map<string, Own>();
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

export function subscribe(listener: () => void): () => void {
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

function register(job: Job): void {
  jobs.set(job.id, job);
  pruneSettled();
  emit();
}

/**
 * The single place a job ends: the first terminal transition wins, so a
 * terminal event and the caller's promise settling together run this once.
 */
function settle(id: string, outcome: Outcome): void {
  const job = jobs.get(id);
  if (job?.status !== 'running') return;
  log.debug(
    { id, channel: job.channel, status: outcome.status },
    'job settled',
  );
  const own = owned.get(id);
  owned.delete(id);
  const settled = { ...job, ...outcome, settledAt: Date.now() };
  jobs.set(id, settled);
  emit();
  if (outcome.status === 'interrupted')
    own?.abort.abort(new JobInterrupted(id));
  for (const key of own?.sweep ?? []) invalidate(key);
  announce(settled, outcome, own);
}

/**
 * The one reporter of a job's end, keyed `job:<id>` so it never repeats. Every
 * outcome worth knowing lands in the notification list; only an own job the
 * user did not watch finish also toasts. A cancel says nothing: the user asked
 * for it.
 */
function announce(job: Job, outcome: Outcome, own: Own | undefined): void {
  const id = `job:${job.id}`;
  const title = jobTitle(job, entryName(job.subject));
  const watched = !job.background;
  if (outcome.status === 'interrupted') {
    notify.warning(m['app.jobs.interrupted']({ title }), {
      id,
      description: m['app.jobs.interrupted_detail'](),
      quiet: true,
    });
    return;
  }
  if (outcome.status === 'failed') {
    notify.failure(outcome.error, {
      id,
      title: m['app.jobs.failed_title']({ title }),
      action: own?.retry && {
        label: m['app.toast.retry'](),
        onClick: own.retry,
      },
      quiet: !own || (watched && job.ownsError),
    });
    return;
  }
  if (outcome.status !== 'done') return;
  const described = own?.notify?.(outcome.done);
  if (!described) return;
  const unrendered =
    !!described.warnings?.length ||
    (!job.ownsError && !!described.failures?.failed);
  notify.outcome({ ...described, id }, { quiet: watched && !unrendered });
}

function adopt(
  id: string,
  channel: string,
  subject: JobEntry | null,
  progress: unknown,
  startedAt: number,
): void {
  log.debug({ id, channel }, 'job adopted');
  register({
    id,
    channel,
    subject,
    target: null,
    origin: 'foreign',
    status: 'running',
    progress: progress ?? null,
    done: null,
    error: null,
    startedAt,
    settledAt: null,
    background: true,
    ownsError: false,
  });
}

function onJobEvent(event: Topics['job']): void {
  if (!jobs.has(event.id)) {
    adopt(event.id, event.channel, event.subject ?? null, null, Date.now());
  }
  switch (event.state) {
    case 'progress':
      if (jobs.get(event.id)?.status === 'running')
        patch(event.id, { progress: event }, true);
      break;
    case 'done':
      settle(event.id, { status: 'done', done: event });
      break;
    case 'cancelled':
      settle(event.id, { status: 'cancelled' });
      break;
    case 'failed':
      settle(event.id, {
        status: 'failed',
        error: new HestiaError(HANDLER_ERROR, event.error.kind, event.error),
      });
      break;
  }
}

/**
 * Reconcile with the daemon's registry: adopt running jobs the store lacks and
 * mark the ones the daemon no longer knows as interrupted (it restarted under
 * them). A job started after the request went out cannot be in the answer, so
 * it is left alone.
 */
async function hydrate(): Promise<void> {
  const requestedAt = Date.now();
  let listed: Awaited<ReturnType<typeof jobApi.list>>;
  try {
    listed = await jobApi.list();
  } catch (error) {
    log.warn({ error }, 'job list failed');
    return;
  }
  const live = new Set(listed.map((info) => info.id));
  for (const info of listed) {
    if (jobs.has(info.id)) continue;
    adopt(
      info.id,
      info.channel,
      info.subject ?? null,
      info.progress,
      info.startedUnix * 1000,
    );
  }
  for (const job of [...jobs.values()]) {
    if (
      job.status === 'running' &&
      !live.has(job.id) &&
      job.startedAt <= requestedAt
    ) {
      settle(job.id, { status: 'interrupted' });
    }
  }
}

/** Install the daemon listeners and read the registry; call once at app start. */
export async function startJobs(): Promise<void> {
  await onTopic('job', onJobEvent);
  await onConnectionChange((state) => {
    if (state === 'connected') void hydrate();
  });
  await hydrate();
}

export interface JobHandle<TData> {
  id: string;
  result: Promise<TData>;
}

/**
 * Run one tracked job: register it, then let the daemon's events drive its
 * progress and state. The run's own settling is the fallback for a job whose
 * start call failed before any event could arrive.
 */
export function startJob<TData>(
  meta: JobMeta,
  run: (job: JobRun) => Promise<TData>,
  options: JobOptions<TData> = {},
): JobHandle<TData> {
  const id = jobId(meta.channel);
  log.debug({ id, channel: meta.channel }, 'job started');
  const abort = new AbortController();
  owned.set(id, {
    sweep: options.invalidates ?? [],
    notify: options.notify as Own['notify'],
    retry: options.retry,
    abort,
  });
  register({
    id,
    channel: meta.channel,
    subject: meta.subject ?? null,
    target: meta.target ?? null,
    origin: 'own',
    status: 'running',
    progress: null,
    done: null,
    error: null,
    startedAt: Date.now(),
    settledAt: null,
    background: true,
    ownsError: false,
  });

  const result = run({ id, signal: abort.signal }).then(
    (data) => {
      settle(id, { status: 'done', done: data });
      return data;
    },
    (error: unknown) => {
      settle(
        id,
        error instanceof JobCancelled
          ? { status: 'cancelled' }
          : { status: 'failed', error: asHestiaError(error) },
      );
      throw error;
    },
  );
  return { id, result };
}

/**
 * Hide a job from the status bar while a modal owns its display;
 * `ownsError` says the modal renders the failure itself.
 */
export function foregroundJob(id: string, ownsError = false): void {
  const job = jobs.get(id);
  if (job?.background === false && job.ownsError === ownsError) return;
  patch(id, { background: false, ownsError });
}

/** Reveal a job in the status bar after its modal is closed. */
export function backgroundJob(id: string): void {
  if (jobs.get(id)?.background === true) return;
  patch(id, { background: true });
}

/** Ask the daemon to stop a running job; it settles on its cancelled event. */
export async function cancelJob(id: string): Promise<void> {
  if (jobs.get(id)?.status !== 'running') return;
  await jobApi.cancel(id);
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
