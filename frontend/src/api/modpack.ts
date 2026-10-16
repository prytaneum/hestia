/**
 * The `modpack.*` channels — installing a pack into a new or existing entry,
 * moving it to another published version, reading which pack an entry runs, and
 * taking it back out.
 *
 * Installing keeps a channel per side, because a server install carries an EULA
 * and a port; every verb on an existing entry names its side in an
 * `EntryTarget`, and an instance target is refused until an account is signed
 * in.
 *
 * Install and update are jobs: a pack is a hundred downloads and a game
 * directory rewrite. Their done event carries the entry the pack landed in,
 * which is the only way a caller learns the id of an entry a *creating* install
 * just made.
 */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type { EntryTarget } from './types/error';
import type {
  InstalledModpack,
  ModpackDoneEvent,
  ModpackFilePreview,
  ModpackMove,
  ModpackRef,
  ModpackRemoveResult,
  ModpackTarget,
  ModpackUpdate,
} from './types/modpack';

const topics = {
  progress: 'modpack.progress',
  done: 'modpack.done',
  error: 'modpack.error',
} as const;

/** Exactly one of `project`, `url`, or `path` names the pack. */
export type PackRef = Partial<ModpackRef>;

export function installInstance(
  pack: PackRef,
  target: ModpackTarget,
  job: JobRun,
): Promise<ModpackDoneEvent> {
  return runJob<ModpackDoneEvent>({
    ...job,
    topics,
    start: () =>
      call('instance.modpack.install', { ...pack, target, id: job.id }),
  });
}

/** `eula` is required when the target creates a server. */
export function installServer(
  pack: PackRef,
  target: ModpackTarget,
  options: { eula?: boolean; port?: number },
  job: JobRun,
): Promise<ModpackDoneEvent> {
  return runJob<ModpackDoneEvent>({
    ...job,
    topics,
    start: () =>
      call('server.modpack.install', {
        ...pack,
        target,
        ...options,
        id: job.id,
      }),
  });
}

/**
 * Where an update takes the pack: another published `version` (the newest when
 * empty) or a pack file at `path`. Either carries the entry's game version with
 * it, so a pack built for an older one needs `allowDowngrade`.
 */
export type PackMove = Partial<ModpackMove>;

export function update(
  target: EntryTarget,
  to: PackMove,
  job: JobRun,
): Promise<ModpackDoneEvent> {
  return runJob<ModpackDoneEvent>({
    ...job,
    topics,
    start: () => call('modpack.update', { target, to, id: job.id }),
  });
}

/** What updating from the pack file at `path` would do, before it starts. */
export function previewUpdate(
  target: EntryTarget,
  path: string,
): Promise<ModpackFilePreview> {
  return call(
    'modpack.preview_update',
    { target, path },
    { timeoutMs: 120_000 },
  );
}

/** `null` when the entry was not built from a pack — an ordinary answer. */
export async function status(
  target: EntryTarget,
): Promise<InstalledModpack | null> {
  const result = await call<{ pack?: InstalledModpack }>('modpack.status', {
    target,
  });
  return result.pack ?? null;
}

/** `null` when there is nothing to check: no pack, or one from a file. */
export async function checkUpdate(
  target: EntryTarget,
  gameVersion = '',
): Promise<ModpackUpdate | null> {
  const result = await call<{ update?: ModpackUpdate }>(
    'modpack.check_update',
    { target, gameVersion },
    { timeoutMs: 120_000 },
  );
  return result.update ?? null;
}

export function remove(
  target: EntryTarget,
  keepContent: boolean,
): Promise<ModpackRemoveResult> {
  return call('modpack.remove', { target, keepContent });
}
