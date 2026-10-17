/**
 * The job driver, mirroring the client SDK's `Session::run_job`: generate a
 * job id, listen for its events, start the job, then settle on its terminal
 * state while streaming progress. Every job reports on the one `job` topic;
 * the event handler is installed *before* the start call, so even a job that
 * finishes instantly cannot slip its terminal event past us. No per-job
 * `events.subscribe` is issued: the desktop bridge already holds one
 * connection subscribed to every daemon event, so a second subscription would
 * only duplicate delivery (and leak a daemon-side sub that is pruned only on
 * disconnect).
 */
import { logger } from '@/lib/log';
import type { Channels, Jobs } from '../types/channels';
import type { ProvisionProgress } from '../types/minecraft';
import { onDaemonEvent } from './events';
import { HANDLER_ERROR, HestiaError, request } from './ipc';

const log = logger('job');

export const JOB_TOPIC = 'job';

let counter = 0;

export function jobId(prefix: string): string {
  counter += 1;
  return `${prefix}-${Date.now().toString(36)}-${counter}`;
}

/**
 * The rejection a cancelled job settles with. Distinguished from a failure so a
 * surface can say "cancelled" instead of rendering an error the user caused on
 * purpose.
 */
export class JobCancelled extends Error {
  constructor(readonly id: string) {
    super('cancelled');
    this.name = 'JobCancelled';
  }
}

/**
 * One run's identity, minted by the caller and threaded into the API function.
 * Passed in rather than reported back so the caller's tracking cannot be
 * orphaned by an `await` on the way to `runJob`.
 */
export interface JobRun<TProgress = ProvisionProgress> {
  id: string;
  onProgress?: (progress: TProgress) => void;
}

type JobChannel = keyof Jobs & keyof Channels;

/** A start channel's params without the job id `runJob` supplies itself. */
export type JobParams<K extends JobChannel> = Omit<Channels[K]['params'], 'id'>;

/**
 * Start the job `channel` under `job.id` and run it to completion. Resolves
 * with the done event's payload (the contract's `Done`, carried beside the
 * event's `id`/`channel`/`state`); rejects with a `HestiaError` carrying the
 * failure's structured cause, or `JobCancelled`.
 */
export async function runJob<K extends JobChannel>(
  channel: K,
  params: JobParams<K>,
  job: JobRun<Jobs[K]['progress']>,
): Promise<Jobs[K]['done']> {
  const { id, onProgress } = job;
  let resolveOutcome!: (done: Jobs[K]['done']) => void;
  let rejectOutcome!: (error: Error) => void;
  const outcome = new Promise<Jobs[K]['done']>((resolve, reject) => {
    resolveOutcome = resolve;
    rejectOutcome = reject;
  });

  let settled = false;
  const off = await onDaemonEvent((event) => {
    if (settled || event.topic !== JOB_TOPIC || event.payload.id !== id) return;
    const { payload } = event;
    switch (payload.state) {
      case 'done':
        settled = true;
        resolveOutcome(payload);
        break;
      case 'cancelled':
        settled = true;
        log.debug({ id }, 'job cancelled');
        rejectOutcome(new JobCancelled(id));
        break;
      case 'failed': {
        settled = true;
        // The display site localizes the structured `ErrorInfo`; `kind` is a
        // stable fallback message, shown only if that localization ever misses.
        log.warn({ id, kind: payload.error.kind }, 'job failed');
        rejectOutcome(
          new HestiaError(HANDLER_ERROR, payload.error.kind, payload.error),
        );
        break;
      }
      case 'progress':
        onProgress?.(payload);
        break;
    }
  });

  try {
    log.debug({ id, channel }, 'job started');
    await request(channel, { ...params, id });
    return await outcome;
  } finally {
    off();
  }
}
