/** The `update.*` channels. */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type { DownloadProgress } from './types/download';
import type {
  UpdateApplyResult,
  UpdateCheckResult,
  UpdateDone,
} from './types/update';

export function check(): Promise<UpdateCheckResult> {
  return call('update.check');
}

export function download(job: JobRun<DownloadProgress>): Promise<UpdateDone> {
  return runJob('update.download', {}, job);
}

export function apply(path: string): Promise<UpdateApplyResult> {
  return call('update.apply', { path });
}
