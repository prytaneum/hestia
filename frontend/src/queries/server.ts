/**
 * `server.*` — the `serverQueries`/`serverMutations` factories, consumed
 * directly through useQuery/useMutation/useJobMutation. Only the hooks that
 * add real behavior stay named (below). Per-entry factories take the server's
 * **stable id**; the wire resolves display names too, but keying by id means a
 * rename never strands a cache entry or a mutation key.
 */
import {
  keepPreviousData,
  queryOptions,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import type {
  BackupInfo,
  ConfigEntry,
  ContentKind,
  ResolveParams,
  ServerCreateDoneEvent,
  ServerCreateParams,
  ServerInfo,
  ServerUpdateDoneEvent,
  ServerUpdateParams,
} from '../api';
import * as api from '../api/server';
import { CATALOG_STALE_MS, mutation } from './core';
import { entryContentFactories } from './entry-content';
import { useEntryIconLookup } from './icons';
import { jobMutation } from './jobs';
import { keys } from './keys';
import { type LogsOptions, type LogsResult, useFollowedLogs } from './logs';

export const serverQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.servers.list(),
      queryFn: () => api.list(),
    }),
  detail: (id: string) =>
    queryOptions({
      queryKey: keys.servers.detail(id),
      queryFn: () => api.status(id),
    }),
  // The informational view (locations + footprint) is a directory walk, so it
  // rides its own key — fetched fresh (never seeded from the diskless list) and
  // off the frequently-invalidated detail query.
  info: (id: string) =>
    queryOptions({
      queryKey: keys.servers.info(id),
      queryFn: () => api.info(id),
      staleTime: 60_000,
    }),
  ping: (id: string) =>
    queryOptions({
      queryKey: keys.servers.ping(id),
      queryFn: () => api.ping(id),
      meta: { silent: true },
      // Live players/MOTD while enabled; the caller gates on `running`.
      refetchInterval: 5000,
      retry: false,
    }),
  // Asked per keystroke; the caller gates on `running` and a non-empty line.
  complete: (id: string, line: string) =>
    queryOptions({
      queryKey: keys.servers.complete(id, line),
      queryFn: () => api.complete(id, line),
      meta: { silent: true },
      placeholderData: keepPreviousData,
      gcTime: 0,
      retry: false,
    }),
  flavors: () =>
    queryOptions({
      queryKey: keys.servers.flavors(),
      queryFn: () => api.flavors(),
      staleTime: CATALOG_STALE_MS,
    }),
  versions: (flavor: string) =>
    queryOptions({
      queryKey: keys.servers.versions(flavor),
      queryFn: () => api.versions(flavor),
      staleTime: CATALOG_STALE_MS,
    }),
  loaders: (flavor: string, version: string) =>
    queryOptions({
      queryKey: keys.servers.loaders(flavor, version),
      queryFn: () => api.loaders(flavor, version),
      staleTime: CATALOG_STALE_MS,
    }),
  profile: (params: ResolveParams) =>
    queryOptions({
      queryKey: keys.servers.profile(params),
      queryFn: () => api.resolve(params),
      staleTime: CATALOG_STALE_MS,
    }),
  logs: (id: string, tail?: number) =>
    queryOptions({
      queryKey: keys.servers.logs(id, tail),
      queryFn: () => api.logs(id, tail),
    }),
  config: (id: string) =>
    queryOptions({
      queryKey: keys.servers.config(id),
      queryFn: () => api.config.list(id),
    }),
  /** Nested under the config key, so a `config.set` refreshes it too. */
  properties: (id: string) =>
    queryOptions({
      queryKey: keys.servers.properties(id),
      queryFn: () => api.properties(id),
    }),
  backups: (id: string) =>
    queryOptions({
      queryKey: keys.servers.backups(id),
      queryFn: () => api.backup.list(id),
    }),
  content: (id: string, kind: ContentKind) =>
    queryOptions({
      queryKey: keys.servers.contentList(id, kind),
      queryFn: () => api.content.list(id, kind),
    }),
  contentMatches: (id: string, kind: ContentKind, item: string) =>
    queryOptions({
      queryKey: keys.servers.contentMatches(id, kind, item),
      queryFn: () => api.content.matches(id, kind, item),
      enabled: item.length > 0,
    }),
  contentUpdates: (id: string, kind: ContentKind) =>
    queryOptions({
      queryKey: keys.servers.contentUpdates(id, kind),
      queryFn: () => api.content.checkUpdates(id, kind),
      // A network resolve per item — refetch only when explicitly asked.
      staleTime: Number.POSITIVE_INFINITY,
      enabled: false,
    }),
};

export const serverMutations = {
  create: () =>
    jobMutation<ServerCreateDoneEvent, Partial<ServerCreateParams>>({
      mutationKey: [...keys.servers.all, 'create'],
      meta: (params) => ({
        kind: 'server.create',
        label: `create ${params.name || `${params.flavor} ${params.version}`}`,
      }),
      run: (params, job) => api.create(params, job),
      invalidates: () => [keys.servers.list()],
    }),
  update: (id: string) =>
    jobMutation<
      ServerUpdateDoneEvent,
      Omit<ServerUpdateParams, 'server' | 'id'>
    >({
      mutationKey: [...keys.servers.detail(id), 'update'],
      meta: (params) => ({
        kind: 'server.update',
        label: `update to ${params.version}`,
        entry: { kind: 'server', id },
      }),
      run: (params, job) => api.update({ ...params, server: id }, job),
      invalidates: () => [
        keys.servers.list(),
        keys.servers.detail(id),
        keys.servers.info(id),
      ],
    }),
  rename: (id: string) =>
    mutation<ServerInfo, string>({
      mutationKey: [...keys.servers.detail(id), 'rename'],
      mutationFn: (name) => api.rename(id, name),
      invalidates: () => [
        keys.servers.list(),
        keys.servers.detail(id),
        keys.servers.info(id),
      ],
    }),
  remove: (id: string) =>
    mutation({
      mutationKey: [...keys.servers.detail(id), 'remove'],
      mutationFn: () => api.remove(id),
      invalidates: () => [keys.servers.list()],
    }),
  start: (id: string) =>
    mutation<{ processId: string; pid: number }>({
      mutationKey: [...keys.servers.detail(id), 'start'],
      mutationFn: () => api.start(id),
      invalidates: () => [keys.servers.list(), keys.servers.detail(id)],
    }),
  stop: (id: string) =>
    mutation({
      mutationKey: [...keys.servers.detail(id), 'stop'],
      mutationFn: () => api.stop(id),
      invalidates: () => [keys.servers.list(), keys.servers.detail(id)],
    }),
  restart: (id: string) =>
    mutation<{ processId: string; pid: number }>({
      mutationKey: [...keys.servers.detail(id), 'restart'],
      mutationFn: () => api.restart(id),
      invalidates: () => [keys.servers.list(), keys.servers.detail(id)],
    }),
  /** Id-by-variable variants for list rows, which can't call a per-id hook. */
  startAny: () =>
    mutation<{ processId: string; pid: number }, string>({
      mutationKey: [...keys.servers.all, 'start'],
      mutationFn: (id) => api.start(id),
      invalidates: (id) => [keys.servers.list(), keys.servers.detail(id)],
    }),
  stopAny: () =>
    mutation<void, string>({
      mutationKey: [...keys.servers.all, 'stop'],
      mutationFn: (id) => api.stop(id),
      invalidates: (id) => [keys.servers.list(), keys.servers.detail(id)],
    }),
  /** One console command over RCON; touches no cached state. */
  command: (id: string) =>
    mutation<string, string>({
      mutationKey: [...keys.servers.detail(id), 'command'],
      mutationFn: (line) => api.command(id, line),
    }),
  setConfig: (id: string) =>
    mutation<void, ConfigEntry>({
      mutationKey: [...keys.servers.detail(id), 'config', 'set'],
      mutationFn: ({ key, value }) => api.config.set(id, key, value),
      invalidates: () => [keys.servers.config(id)],
    }),
  backup: {
    /** Archives a running server live (world saving pauses over RCON). */
    create: (id: string) =>
      jobMutation<BackupInfo>({
        mutationKey: [...keys.servers.backups(id), 'create'],
        meta: () => ({
          kind: 'backup.create',
          label: 'back up',
          entry: { kind: 'server', id },
        }),
        run: (_variables, job) => api.backup.create(id, job),
        invalidates: () => [keys.servers.backups(id), keys.servers.info(id)],
      }),
    /** Refused while the server runs or is busy; swaps `data/` wholesale. */
    restore: (id: string) =>
      jobMutation<BackupInfo, string>({
        mutationKey: [...keys.servers.backups(id), 'restore'],
        meta: () => ({
          kind: 'backup.restore',
          label: 'restore backup',
          entry: { kind: 'server', id },
        }),
        run: (backupId, job) => api.backup.restore(id, backupId, job),
        invalidates: () => [keys.servers.detail(id), keys.servers.info(id)],
      }),
    remove: (id: string) =>
      mutation<void, string>({
        mutationKey: [...keys.servers.backups(id), 'remove'],
        mutationFn: (backupId) => api.backup.remove(id, backupId),
        invalidates: () => [keys.servers.backups(id), keys.servers.info(id)],
      }),
  },
  /** Servers take mods and datapacks; refused on a running or busy server. */
  content: entryContentFactories({
    kind: 'server',
    api: api.content,
    contentKey: keys.servers.content,
    infoKey: keys.servers.info,
  }),
};

export function useServers() {
  const iconFor = useEntryIconLookup();
  return useQuery({
    ...serverQueries.list(),
    select: (servers) =>
      servers.map((server) => ({ ...server, iconUrl: iconFor(server.id) })),
  });
}

/** One server's status, seeded from the list cache so a row costs no call. */
export function useServer(id: string) {
  const queryClient = useQueryClient();
  const iconFor = useEntryIconLookup();
  return useQuery({
    ...serverQueries.detail(id),
    initialData: () =>
      queryClient
        .getQueryData<ServerInfo[]>(keys.servers.list())
        ?.find((server) => server.id === id),
    initialDataUpdatedAt: () =>
      queryClient.getQueryState(keys.servers.list())?.dataUpdatedAt,
    select: (server) => ({ ...server, iconUrl: iconFor(server.id) }),
  });
}

/**
 * Live logs — a fetched tail plus streamed output. Kept as a hook because it
 * composes the follow matcher; every other read/write is consumed directly
 * through `serverQueries`/`serverMutations` (with `useQuery`/`useMutation`/
 * `useJobMutation`, spreading `{ enabled }` at the call site when gated).
 *
 * Following is scoped to the server, not to one run of it: leave `follow` on
 * while it is stopped and the stream resumes on the next start.
 */
export function useServerLogs(
  id: string,
  options: LogsOptions = {},
): LogsResult {
  const query = useQuery({
    ...serverQueries.logs(id, options.tail),
    staleTime: options.follow ? Number.POSITIVE_INFINITY : undefined,
  });
  return useFollowedLogs(
    query,
    options.follow ? (processId) => processId === `server-${id}` : null,
    options.limit,
  );
}
