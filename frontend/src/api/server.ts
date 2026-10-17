/**
 * The `server.*` channels. Every per-entry call names the server by display
 * name or id (`proto::naming` resolves either). `create` and `update` are
 * jobs — the returned promise settles on the job's terminal event while
 * progress streams through the callback; backups are jobs too.
 */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type { BackupDone, BackupInfo } from './types/backup';
import type {
  ConfigEntry,
  FlavorsResult,
  GameVersion,
} from './types/minecraft';
import type { ProcessLogLine } from './types/process';
import type {
  ServerCommandCompleteResult,
  ServerCreateParams,
  ServerDetails,
  ServerDone,
  ServerInfo,
  ServerPingResult,
  ServerProperty,
  ServerUpdateParams,
} from './types/server';

export function flavors(): Promise<FlavorsResult> {
  return call<FlavorsResult>('server.flavors');
}

export async function versions(flavor: string): Promise<GameVersion[]> {
  const result = await call<{ versions: GameVersion[] }>('server.versions', {
    flavor,
  });
  return result.versions;
}

/** Loader builds for a flavor/version, newest first; empty for vanilla. */
export async function loaders(
  flavor: string,
  version: string,
): Promise<string[]> {
  const result = await call<{ loaders: string[] }>('server.loaders', {
    flavor,
    version,
  });
  return result.loaders;
}

export async function list(): Promise<ServerInfo[]> {
  const result = await call<{ servers: ServerInfo[] }>('server.list');
  return result?.servers ?? [];
}

/** The server's record merged with its live process state. */
export function status(server: string): Promise<ServerInfo> {
  return call('server.status', { server });
}

/** The server's static, informational view (locations + disk footprint). */
export function info(server: string): Promise<ServerDetails> {
  return call('server.info', { server });
}

/** A Server List Ping snapshot; only a running server answers. */
export function ping(server: string): Promise<ServerPingResult> {
  return call('server.ping', { server });
}

export async function create(
  params: Partial<ServerCreateParams>,
  job: JobRun,
): Promise<ServerDone> {
  return await runJob<ServerDone>({
    ...job,
    start: () => call('server.create', { ...params, id: job.id }),
  });
}

export async function update(
  params: Omit<ServerUpdateParams, 'id'>,
  job: JobRun,
): Promise<ServerDone> {
  return await runJob<ServerDone>({
    ...job,
    start: () => call('server.update', { ...params, id: job.id }),
  });
}

export function rename(server: string, name: string): Promise<ServerInfo> {
  return call('server.rename', { server, name });
}

export async function remove(server: string): Promise<void> {
  await call('server.remove', { server });
}

export function start(
  server: string,
): Promise<{ processId: string; pid: number }> {
  return call('server.start', { server });
}

export async function stop(server: string): Promise<void> {
  await call('server.stop', { server });
}

/** Answers once the new process is up, so it waits out the old one's stop. */
export function restart(
  server: string,
): Promise<{ processId: string; pid: number }> {
  return call('server.restart', { server }, { timeoutMs: 150_000 });
}

export async function logs(
  server: string,
  tail?: number,
): Promise<ProcessLogLine[]> {
  const result = await call<{ lines: ProcessLogLine[] }>('server.logs', {
    server,
    tail,
  });
  return result.lines;
}

/** One console command over the running server's RCON channel. */
export async function command(
  server: string,
  commandLine: string,
): Promise<string> {
  const result = await call<{ response: string }>('server.command', {
    server,
    command: commandLine,
  });
  return result.response;
}

/** What a half-typed console line could continue with on a running server. */
export function complete(
  server: string,
  line: string,
): Promise<ServerCommandCompleteResult> {
  return call<ServerCommandCompleteResult>('server.command.complete', {
    server,
    line,
  });
}

/**
 * The record keys (`memory`, `jvm-args`, `backup-interval`,
 * `backup-retention`) plus any `server.properties` key.
 */
export const config = {
  async set(server: string, key: string, value: string): Promise<void> {
    await call('server.config.set', { server, key, value });
  },

  async list(server: string): Promise<ConfigEntry[]> {
    const result = await call<{ entries: ConfigEntry[] }>(
      'server.config.list',
      { server },
    );
    return result.entries;
  },
};

/**
 * Every `server.properties` key the server's version defines, with the value
 * in effect. Written through `config.set`.
 */
export async function properties(server: string): Promise<ServerProperty[]> {
  const result = await call<{ properties: ServerProperty[] }>(
    'server.properties',
    { server },
  );
  return result.properties;
}

export const backup = {
  /** Archives a running server live (world saving pauses over RCON). */
  async create(server: string, job: JobRun): Promise<BackupInfo> {
    const done = await runJob<BackupDone>({
      ...job,
      start: () => call('server.backup.create', { server, id: job.id }),
    });
    return done.backup;
  },

  async list(server: string): Promise<BackupInfo[]> {
    const result = await call<{ backups: BackupInfo[] }>('server.backup.list', {
      server,
    });
    return result.backups;
  },

  /** Refused while the server runs or is busy. */
  async restore(
    server: string,
    backupId: string,
    job: JobRun,
  ): Promise<BackupInfo> {
    const done = await runJob<BackupDone>({
      ...job,
      start: () =>
        call('server.backup.restore', { server, backup: backupId, id: job.id }),
    });
    return done.backup;
  },

  async remove(server: string, backupId: string): Promise<void> {
    await call('server.backup.remove', { server, backup: backupId });
  },
};
