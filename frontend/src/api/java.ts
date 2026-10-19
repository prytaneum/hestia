/** The `java.*` channels. */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type { JavaInstallDone, JavaRelease, JavaRuntime } from './types/java';

export function releases(): Promise<JavaRelease[]> {
  return call('java.releases');
}

export function list(): Promise<JavaRuntime[]> {
  return call('java.list');
}

export function install(
  major: number,
  options: { force?: boolean },
  job: JobRun,
): Promise<JavaInstallDone> {
  return runJob('java.install', { major, force: options.force ?? false }, job);
}

export async function uninstall(major: number): Promise<void> {
  await call('java.uninstall', { major });
}
