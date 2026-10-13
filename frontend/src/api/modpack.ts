/**
 * The `*.modpack.*` channels — installing a pack into a new or existing entry,
 * moving it to another published version, reading which pack an entry runs, and
 * taking it back out.
 *
 * Install and update are jobs: a pack is a hundred downloads and a game
 * directory rewrite. Their done event carries the entry the pack landed in,
 * which is the only way a caller learns the id of an entry a *creating* install
 * just made.
 */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
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

export function updateInstance(
  instance: string,
  to: PackMove,
  job: JobRun,
): Promise<ModpackDoneEvent> {
  return runJob<ModpackDoneEvent>({
    ...job,
    topics,
    start: () => call('instance.modpack.update', { instance, to, id: job.id }),
  });
}

export function updateServer(
  server: string,
  to: PackMove,
  job: JobRun,
): Promise<ModpackDoneEvent> {
  return runJob<ModpackDoneEvent>({
    ...job,
    topics,
    start: () => call('server.modpack.update', { server, to, id: job.id }),
  });
}

/** What updating from the pack file at `path` would do, before it starts. */
export function previewInstanceUpdate(
  instance: string,
  path: string,
): Promise<ModpackFilePreview> {
  return call(
    'instance.modpack.preview_update',
    { instance, path },
    { timeoutMs: 120_000 },
  );
}

export function previewServerUpdate(
  server: string,
  path: string,
): Promise<ModpackFilePreview> {
  return call(
    'server.modpack.preview_update',
    { server, path },
    { timeoutMs: 120_000 },
  );
}

/** `null` when the entry was not built from a pack — an ordinary answer. */
export async function instanceStatus(
  instance: string,
): Promise<InstalledModpack | null> {
  const result = await call<{ pack?: InstalledModpack }>(
    'instance.modpack.status',
    { instance },
  );
  return result.pack ?? null;
}

export async function serverStatus(
  server: string,
): Promise<InstalledModpack | null> {
  const result = await call<{ pack?: InstalledModpack }>(
    'server.modpack.status',
    { server },
  );
  return result.pack ?? null;
}

/** `null` when there is nothing to check: no pack, or one from a file. */
export async function instanceCheckUpdate(
  instance: string,
  gameVersion = '',
): Promise<ModpackUpdate | null> {
  const result = await call<{ update?: ModpackUpdate }>(
    'instance.modpack.check_update',
    { instance, gameVersion },
    { timeoutMs: 120_000 },
  );
  return result.update ?? null;
}

export async function serverCheckUpdate(
  server: string,
  gameVersion = '',
): Promise<ModpackUpdate | null> {
  const result = await call<{ update?: ModpackUpdate }>(
    'server.modpack.check_update',
    { server, gameVersion },
    { timeoutMs: 120_000 },
  );
  return result.update ?? null;
}

export function removeInstance(
  instance: string,
  keepContent: boolean,
): Promise<ModpackRemoveResult> {
  return call('instance.modpack.remove', { instance, keepContent });
}

export function removeServer(
  server: string,
  keepContent: boolean,
): Promise<ModpackRemoveResult> {
  return call('server.modpack.remove', { server, keepContent });
}
