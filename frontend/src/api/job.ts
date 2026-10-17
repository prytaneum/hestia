/**
 * `job.cancel` — stopping a long-running daemon operation.
 *
 * One channel for every job family, because a job is identified by the id its
 * own progress and terminal events carry. A job outlives the client that
 * started it, so nothing cancels implicitly: closing a page or losing the
 * connection leaves the work running, and only this asks it to stop.
 */
import { call } from './core/ipc';

/**
 * Ask the daemon to cancel a running job. `false` means it was already over —
 * a normal race when the user clicks as it finishes, not an error.
 */
export async function cancel(id: string): Promise<boolean> {
  const result = await call('job.cancel', { id });
  return result.cancelled;
}
