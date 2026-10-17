/**
 * The `server.*` channels. Every per-entry call names the server by display
 * name or id (`proto::naming` resolves either). `create` and `update` are
 * jobs — the returned promise settles on the job's terminal event while
 * progress streams through the callback; backups are jobs too.
 */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type { BackupInfo } from './types/backup';
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
  ServerStartResult,
  ServerUpdateParams,
} from './types/server';

export function flavors(): Promise<FlavorsResult> {
  return call('server.flavors');
}

export function versions(flavor: string): Promise<GameVersion[]> {
  return call('server.versions', { flavor });
}

/** Loader builds for a flavor/version, newest first; empty for vanilla. */
export function loaders(flavor: string, version: string): Promise<string[]> {
  return call('server.loaders', { flavor, version });
}

export function list(): Promise<ServerInfo[]> {
  return call('server.list');
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

export function create(
  params: Partial<ServerCreateParams>,
  job: JobRun,
): Promise<ServerDone> {
  return runJob('server.create', params, job);
}

export function update(
  params: Partial<Omit<ServerUpdateParams, 'id'>>,
  job: JobRun,
): Promise<ServerDone> {
  return runJob('server.update', params, job);
}

export function rename(server: string, name: string): Promise<ServerInfo> {
  return call('server.rename', { server, name });
}

export async function remove(server: string): Promise<void> {
  await call('server.remove', { server });
}

export function start(server: string): Promise<ServerStartResult> {
  return call('server.start', { server });
}

export async function stop(server: string): Promise<void> {
  await call('server.stop', { server });
}

export function restart(server: string): Promise<ServerStartResult> {
  return call('server.restart', { server });
}

export function logs(server: string, tail?: number): Promise<ProcessLogLine[]> {
  return call('server.logs', { server, tail });
}

/** One console command over the running server's RCON channel. */
export async function command(
  server: string,
  commandLine: string,
): Promise<string> {
  const result = await call('server.command', {
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
  return call('server.command.complete', { server, line });
}

/**
 * The record keys (`memory`, `jvm-args`) plus any `server.properties` key.
 */
export const config = {
  async set(server: string, key: string, value: string): Promise<void> {
    await call('server.config.set', { server, key, value });
  },

  async list(server: string): Promise<ConfigEntry[]> {
    const result = await call('server.config.list', { server });
    return result.entries;
  },
};

/**
 * Every `server.properties` key the server's version defines, with the value
 * in effect. Written through `config.set`.
 */
export function properties(server: string): Promise<ServerProperty[]> {
  return call('server.properties', { server });
}

export const backup = {
  /** Archives a running server live (world saving pauses over RCON). */
  async create(server: string, job: JobRun): Promise<BackupInfo> {
    const done = await runJob('server.backup.create', { server }, job);
    return done.backup;
  },

  list(server: string): Promise<BackupInfo[]> {
    return call('server.backup.list', { server });
  },

  /** Refused while the server runs or is busy. */
  async restore(
    server: string,
    backupId: string,
    job: JobRun,
  ): Promise<BackupInfo> {
    const done = await runJob(
      'server.backup.restore',
      { server, backup: backupId },
      job,
    );
    return done.backup;
  },

  async remove(server: string, backupId: string): Promise<void> {
    await call('server.backup.remove', { server, backup: backupId });
  },
};
