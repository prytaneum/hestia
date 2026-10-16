/**
 * An instance's live sessions. One instance can be launched several times
 * concurrently; each launch is a supervised process keyed `instance-<id>_<seq>`
 * (`proto::naming`).
 */
import type { InstanceInfo, ProcessInfo } from '@/api';
import { getLocale } from '@/paraglide/runtime.js';

/** The instance's running sessions, oldest launch first. */
export function runningSessions(instance: InstanceInfo): ProcessInfo[] {
  return (instance.sessions ?? [])
    .filter((session) => session.state === 'running')
    .sort((a, b) => a.startedUnix - b.startedUnix);
}

/**
 * The clock time a session started, which is what names it. The key's seq is
 * never reused while a finished session's logs are retained, so it reads as a
 * launch counter rather than a name.
 */
export function sessionStart(session: ProcessInfo): string {
  return new Date(session.startedUnix * 1000).toLocaleTimeString(getLocale(), {
    hour: '2-digit',
    minute: '2-digit',
  });
}
