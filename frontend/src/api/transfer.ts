/**
 * The instance import/export channels — moving an instance out of the launcher
 * as one file, and bringing one back.
 *
 * Both are jobs: an archive is an arbitrary number of files, and an import may
 * download every mod a pack names. `inspect` is the cheap read that says what a
 * file *is* before any of that starts, and `contents` is the tree an export's
 * exclusions are picked out of.
 */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type {
  ArchiveEntry,
  ArchiveInfo,
  ExportDone,
  ExportFormat,
  ImportDone,
} from './types/transfer';

/** What an export of this instance would carry, sorted by path. */
export async function contents(instance: string): Promise<ArchiveEntry[]> {
  const result = await call('instance.export.contents', { instance });
  return result.entries;
}

/** What an archive is, without importing it. The path must be absolute. */
export function inspect(path: string): Promise<ArchiveInfo> {
  return call('instance.import.inspect', { path });
}

export interface ExportInput {
  format: ExportFormat;
  /**
   * An absolute file path, an absolute directory (a name is generated inside
   * it), or empty for the daemon's own `exports/`. The daemon refuses a
   * relative path — it does not share the window's working directory.
   */
  destination?: string;
  /** Entry-relative paths to leave out, from {@link contents}. */
  exclude?: string[];
}

export function exportInstance(
  instance: string,
  input: ExportInput,
  job: JobRun,
): Promise<ExportDone> {
  return runJob(
    'instance.export',
    {
      instance,
      format: input.format,
      destination: input.destination ?? '',
      exclude: input.exclude ?? [],
    },
    job,
  );
}

/**
 * Import an archive as a new instance. The format is detected from the file, so
 * this takes a path and, optionally, a name to override the archive's own.
 */
export function importInstance(
  path: string,
  name = '',
  job: JobRun,
): Promise<ImportDone> {
  return runJob('instance.import', { path, name }, job);
}
