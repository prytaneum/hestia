/** The `java.*` channels. */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type {
  JavaInstallDone,
  JavaInstallProgress,
  JavaRelease,
  JavaRuntime,
} from './types/java';

export async function releases(): Promise<JavaRelease[]> {
  const result = await call('java.releases');
  return result.releases;
}

export async function list(): Promise<JavaRuntime[]> {
  const result = await call('java.list');
  return result.runtimes;
}

export function install(
  major: number,
  options: { force?: boolean },
  job: JobRun<JavaInstallProgress>,
): Promise<JavaInstallDone> {
  return runJob('java.install', { major, force: options.force ?? false }, job);
}

export async function uninstall(major: number): Promise<void> {
  await call('java.uninstall', { major });
}
