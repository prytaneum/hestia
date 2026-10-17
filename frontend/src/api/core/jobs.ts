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
import type { ErrorInfo } from '../types/error';
import type { ProvisionProgress } from '../types/minecraft';
import { onDaemonEvent } from './events';
import { HANDLER_ERROR, HestiaError } from './ipc';

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

export interface JobOptions<TProgress = ProvisionProgress>
  extends JobRun<TProgress> {
  /** The call that starts the job on the daemon. */
  start: () => Promise<unknown>;
}

/**
 * Run one daemon job to completion. Resolves with the done event's payload;
 * rejects with a `HestiaError` carrying the failure's structured cause.
 */
export async function runJob<TDone, TProgress = ProvisionProgress>(
  options: JobOptions<TProgress>,
): Promise<TDone> {
  const { id, onProgress } = options;
  let resolveOutcome!: (done: TDone) => void;
  let rejectOutcome!: (error: HestiaError) => void;
  const outcome = new Promise<TDone>((resolve, reject) => {
    resolveOutcome = resolve;
    rejectOutcome = reject;
  });

  let settled = false;
  const off = await onDaemonEvent((event) => {
    if (settled || event.topic !== JOB_TOPIC || event.payload.id !== id) return;
    switch (event.payload.state) {
      case 'done':
        settled = true;
        resolveOutcome(event.payload as TDone);
        break;
      case 'cancelled':
        settled = true;
        log.debug({ id }, 'job cancelled');
        rejectOutcome(new JobCancelled(id) as unknown as HestiaError);
        break;
      case 'failed': {
        settled = true;
        // The display site localizes the structured `ErrorInfo`; `kind` is a
        // stable fallback message, shown only if that localization ever misses.
        const info = (event.payload.error ?? null) as ErrorInfo | null;
        log.warn({ id, kind: info?.kind }, 'job failed');
        rejectOutcome(
          new HestiaError(HANDLER_ERROR, info?.kind ?? 'job failed', info),
        );
        break;
      }
      case 'progress':
        onProgress?.(event.payload as unknown as TProgress);
        break;
    }
  });

  try {
    log.debug({ id }, 'job started');
    await options.start();
    return await outcome;
  } finally {
    off();
  }
}
