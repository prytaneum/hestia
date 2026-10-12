/**
 * The `server.*` channels. Every per-entry call names the server by display
 * name or id (`proto::naming` resolves either). `create` and `update` are
 * jobs — the returned promise settles on the job's terminal event while
 * progress streams through the callback; backups and content installs share
 * the `backup.*` / `content.*` topics, disambiguated by job id.
 */

import type { ContentAddInput } from './content';
import { call, tryCall } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type { BackupInfo } from './types/backup';
import type {
  ContentDoneEvent,
  ContentKind,
  ContentListResult,
  ContentMatch,
  ContentUpdate,
} from './types/content';
import type {
  ConfigEntry,
  Flavor,
  GameVersion,
  ResolveParams,
  ServerProfile,
} from './types/minecraft';
import type { ProcessLogLine } from './types/process';
import type {
  ServerCreateDoneEvent,
  ServerCreateParams,
  ServerDetails,
  ServerInfo,
  ServerPingResult,
  ServerProperty,
  ServerUpdateDoneEvent,
  ServerUpdateParams,
} from './types/server';

export async function flavors(): Promise<Flavor[]> {
  const result = await call<{ flavors: Flavor[] }>('server.flavors');
  return result.flavors;
}

export async function versions(flavor: string): Promise<GameVersion[]> {
  const result = await call<{ versions: GameVersion[] }>('server.versions', {
    flavor,
  });
  return result.versions;
}

export function resolve(params: ResolveParams): Promise<ServerProfile> {
  return call('server.resolve', params);
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
): Promise<ServerCreateDoneEvent> {
  return await runJob<ServerCreateDoneEvent>({
    ...job,
    topics: {
      progress: 'server.create.progress',
      done: 'server.create.done',
      error: 'server.create.error',
    },
    start: () => call('server.create', { ...params, id: job.id }),
  });
}

export async function update(
  params: Omit<ServerUpdateParams, 'id'>,
  job: JobRun,
): Promise<ServerUpdateDoneEvent> {
  return await runJob<ServerUpdateDoneEvent>({
    ...job,
    topics: {
      progress: 'server.update.progress',
      done: 'server.update.done',
      error: 'server.update.error',
    },
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

/**
 * The record keys (`memory`, `jvm-args`, `backup-interval`,
 * `backup-retention`) plus any `server.properties` key.
 */
export const config = {
  async get(server: string, key: string): Promise<string | null> {
    const result = await tryCall<{ value: string }>('server.config.get', {
      server,
      key,
    });
    return result?.value ?? null;
  },

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
    const done = await runJob<{ id: string; backup: BackupInfo }>({
      ...job,
      topics: { done: 'backup.done', error: 'backup.error' },
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
    const done = await runJob<{ id: string; backup: BackupInfo }>({
      ...job,
      topics: { done: 'backup.done', error: 'backup.error' },
      start: () =>
        call('server.backup.restore', { server, backup: backupId, id: job.id }),
    });
    return done.backup;
  },

  async remove(server: string, backupId: string): Promise<void> {
    await call('server.backup.remove', { server, backup: backupId });
  },
};

export const content = {
  /** Servers take mods and datapacks; refused on a running or busy server. */
  add(
    server: string,
    spec: ContentAddInput,
    job: JobRun,
  ): Promise<ContentDoneEvent> {
    return runJob<ContentDoneEvent>({
      ...job,
      topics: {
        progress: 'content.progress',
        done: 'content.done',
        error: 'content.error',
      },
      start: () => call('server.content.add', { server, ...spec, id: job.id }),
    });
  },

  list(server: string, kind: ContentKind): Promise<ContentListResult> {
    return call('server.content.list', { server, kind });
  },

  /** Uninstalls the named items in one call; a name matching nothing takes none. */
  async remove(
    server: string,
    kind: ContentKind,
    items: string[],
    worlds: string[] = [],
  ): Promise<void> {
    await call('server.content.remove', { server, kind, items, worlds });
  },

  /** `items` empty updates every platform-sourced item of the kind. */
  update(
    server: string,
    kind: ContentKind,
    items: string[],
    job: JobRun,
  ): Promise<ContentDoneEvent> {
    return runJob<ContentDoneEvent>({
      ...job,
      topics: {
        progress: 'content.progress',
        done: 'content.done',
        error: 'content.error',
      },
      start: () =>
        call('server.content.update', { server, kind, items, id: job.id }),
    });
  },

  /** Enable or disable one installed item; applies at the next start. */
  async enable(
    server: string,
    kind: ContentKind,
    item: string,
    enabled: boolean,
    worlds: string[] = [],
  ): Promise<void> {
    await call('server.content.enable', {
      server,
      kind,
      item,
      enabled,
      worlds,
    });
  },

  /** Which platform items of the kind have a newer compatible version. */
  async checkUpdates(
    server: string,
    kind: ContentKind,
  ): Promise<ContentUpdate[]> {
    const result = await call<{ updates: ContentUpdate[] }>(
      'server.content.check_updates',
      { server, kind },
      { timeoutMs: 120_000 },
    );
    return result.updates;
  },

  async matches(
    server: string,
    kind: ContentKind,
    item: string,
  ): Promise<ContentMatch[]> {
    const result = await call<{ matches: ContentMatch[] }>(
      'server.content.matches',
      { server, kind, item },
      { timeoutMs: 120_000 },
    );
    return result.matches;
  },

  /** Re-pin one item to a specific published version (id or number). */
  setVersion(
    server: string,
    kind: ContentKind,
    item: string,
    version: string,
    source: string,
    job: JobRun,
  ): Promise<ContentDoneEvent> {
    return runJob<ContentDoneEvent>({
      ...job,
      topics: {
        progress: 'content.progress',
        done: 'content.done',
        error: 'content.error',
      },
      start: () =>
        call('server.content.set_version', {
          server,
          kind,
          item,
          version,
          source,
          id: job.id,
        }),
    });
  },
};
