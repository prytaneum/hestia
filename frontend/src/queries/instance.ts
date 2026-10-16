/**
 * `instance.*` — the `instanceQueries`/`instanceMutations` factories, consumed
 * directly through useQuery/useMutation/useJobMutation, keyed by stable id like
 * the server factories. Only the behavior-bearing hooks stay named (below):
 * there is no `instance.status` channel, so `useInstance` selects the entry
 * from the list query. `create`/`update` are plain (long) calls, not jobs;
 * `launch` is the job that materialises files and spawns the game.
 */
import { queryOptions, useQuery } from '@tanstack/react-query';
import type {
  ConfigEntry,
  ContentKind,
  InstanceCreateParams,
  InstanceInfo,
  InstanceLaunchDoneEvent,
  InstanceServersWriteResult,
  InstanceUpdateParams,
  QuickPlay,
  ResolveParams,
} from '../api';
import { HestiaError } from '../api';
import * as api from '../api/instance';
import { CATALOG_STALE_MS, mutation } from './core';
import { entryContentFactories } from './entry-content';
import { useEntryIconLookup } from './icons';
import { jobMutation } from './jobs';
import { keys } from './keys';
import { type LogsOptions, type LogsResult, useFollowedLogs } from './logs';

/** A launch, and whether it may run alongside the sessions already up. */
export interface LaunchVars {
  id: string;
  newSession?: boolean;
  quickPlay?: QuickPlay;
}

/** A stop aimed at one session of an instance, or at all of them. */
export interface StopVars {
  id: string;
  session?: string;
}

const EXIT_POLL_MS = 500;
const EXIT_POLLS = 60;

async function sessionExited(id: string, session: string): Promise<void> {
  let name = id;
  for (let poll = 0; poll < EXIT_POLLS; poll++) {
    const instance = (await api.list()).find((entry) => entry.id === id);
    const live = instance?.sessions.some(
      (s) => s.id === session && s.state === 'running',
    );
    if (!live) return;
    name = instance?.name ?? id;
    await new Promise((resolve) => setTimeout(resolve, EXIT_POLL_MS));
  }
  throw new HestiaError('handler_error', `${name} did not stop in time`, {
    kind: 'stop_timed_out',
    name,
  });
}

export const instanceQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.instances.list(),
      queryFn: () => api.list(),
    }),
  /** The worlds, servers and Realms joined lately, across every instance. */
  recent: () =>
    queryOptions({
      queryKey: keys.instances.recent(),
      queryFn: () => api.recent(),
    }),
  // The informational view (locations + footprint) is a directory walk, so it
  // rides its own key — fetched fresh (never seeded from the diskless list).
  info: (id: string) =>
    queryOptions({
      queryKey: keys.instances.info(id),
      queryFn: () => api.info(id),
      staleTime: 60_000,
    }),
  flavors: () =>
    queryOptions({
      queryKey: keys.instances.flavors(),
      queryFn: () => api.flavors(),
      staleTime: CATALOG_STALE_MS,
    }),
  versions: (flavor: string) =>
    queryOptions({
      queryKey: keys.instances.versions(flavor),
      queryFn: () => api.versions(flavor),
      staleTime: CATALOG_STALE_MS,
    }),
  loaders: (flavor: string, version: string) =>
    queryOptions({
      queryKey: keys.instances.loaders(flavor, version),
      queryFn: () => api.loaders(flavor, version),
      staleTime: CATALOG_STALE_MS,
    }),
  profile: (params: ResolveParams) =>
    queryOptions({
      queryKey: keys.instances.profile(params),
      queryFn: () => api.resolve(params),
      staleTime: CATALOG_STALE_MS,
    }),
  /** Save-world folder names, for the datapack world picker. */
  worlds: (id: string) =>
    queryOptions({
      queryKey: keys.instances.worlds(id),
      queryFn: () => api.worlds(id),
    }),
  /** The instance's multiplayer list, in the order the game shows it. */
  servers: (id: string) =>
    queryOptions({
      queryKey: keys.instances.servers(id),
      queryFn: () => api.servers(id),
    }),
  /**
   * What a server answers right now. Its own key so a row refreshes without
   * re-reading the list, and short-lived because a player count is. An address
   * that does not answer is the row's offline state, not an error to report.
   */
  serverStatus: (address: string) =>
    queryOptions({
      queryKey: keys.instances.serverStatus(address),
      queryFn: () => api.pingAddress(address),
      meta: { silent: true },
      staleTime: 30_000,
      retry: false,
    }),
  logs: (id: string, options: { session?: string; tail?: number } = {}) =>
    queryOptions({
      queryKey: keys.instances.logs(id, options.session, options.tail),
      queryFn: () => api.logs(id, options),
    }),
  config: (id: string) =>
    queryOptions({
      queryKey: keys.instances.config(id),
      queryFn: () => api.config.list(id),
    }),
  content: (id: string, kind: ContentKind) =>
    queryOptions({
      queryKey: keys.instances.contentList(id, kind),
      queryFn: () => api.content.list(id, kind),
    }),
  contentMatches: (id: string, kind: ContentKind, item: string) =>
    queryOptions({
      queryKey: keys.instances.contentMatches(id, kind, item),
      queryFn: () => api.content.matches(id, kind, item),
      enabled: item.length > 0,
    }),
  contentUpdates: (id: string, kind: ContentKind) =>
    queryOptions({
      queryKey: keys.instances.contentUpdates(id, kind),
      queryFn: () => api.content.checkUpdates(id, kind),
      // A network resolve per item — refetch only when explicitly asked.
      staleTime: Number.POSITIVE_INFINITY,
      enabled: false,
    }),
};

export const instanceMutations = {
  create: () =>
    mutation<InstanceInfo, Partial<InstanceCreateParams>>({
      mutationKey: [...keys.instances.all, 'create'],
      mutationFn: (params) => api.create(params),
      invalidates: () => [keys.instances.list()],
    }),
  /** The instance pays for the new version at its next launch. */
  update: (id: string) =>
    mutation<InstanceInfo, Omit<InstanceUpdateParams, 'instance' | 'id'>>({
      mutationKey: [...keys.instances.detail(id), 'update'],
      mutationFn: (params) => api.update({ ...params, instance: id }),
      invalidates: () => [
        keys.instances.list(),
        keys.instances.detail(id),
        keys.instances.info(id),
      ],
    }),
  rename: (id: string) =>
    mutation<InstanceInfo, string>({
      mutationKey: [...keys.instances.detail(id), 'rename'],
      mutationFn: (name) => api.rename(id, name),
      invalidates: () => [
        keys.instances.list(),
        keys.instances.detail(id),
        keys.instances.info(id),
      ],
    }),
  remove: (id: string) =>
    mutation({
      mutationKey: [...keys.instances.detail(id), 'remove'],
      mutationFn: () => api.remove(id),
      invalidates: () => [keys.instances.list()],
    }),
  /** Stops one named session, or every session of the instance. */
  stop: (id: string) =>
    mutation<void, { session?: string }>({
      mutationKey: [...keys.instances.detail(id), 'stop'],
      mutationFn: ({ session }) => api.stop(id, session),
      invalidates: () => [keys.instances.list(), keys.instances.detail(id)],
    }),
  /** Adds an entry to the multiplayer list, or rewrites the one `server` names. */
  serverEdit: (id: string) =>
    mutation<
      InstanceServersWriteResult,
      {
        server?: string;
        name: string;
        address: string;
        acceptTextures: boolean;
      }
    >({
      mutationKey: [...keys.instances.servers(id), 'edit'],
      mutationFn: (params) => api.serverEdit({ ...params, instance: id }),
      invalidates: () => [keys.instances.servers(id)],
    }),
  serverRemove: (id: string) =>
    mutation<InstanceServersWriteResult, string>({
      mutationKey: [...keys.instances.servers(id), 'remove'],
      mutationFn: (server) => api.serverRemove(id, server),
      invalidates: () => [keys.instances.servers(id)],
    }),
  /** Commits an arrangement of the whole list, in one write. */
  serversArrange: (id: string) =>
    mutation<InstanceServersWriteResult, string[]>({
      mutationKey: [...keys.instances.servers(id), 'arrange'],
      mutationFn: (order) => api.serversArrange(id, order),
      invalidates: () => [keys.instances.servers(id)],
    }),
  /**
   * Id-by-variable variants for list rows, which can't call a per-id hook. A
   * launch materialises files, so it streams provisioning progress through the
   * job store like the per-id `launch` above. Quick Play is this same job
   * carrying what to join, not a second one.
   *
   * `newSession` is the daemon's concurrency opt-in — without it a launch of a
   * running instance is refused.
   */
  launchAny: () =>
    jobMutation<InstanceLaunchDoneEvent, LaunchVars>({
      mutationKey: [...keys.instances.all, 'launch'],
      meta: ({ id }) => ({
        kind: 'instance.launch',
        label: 'launch',
        entry: { kind: 'instance', id },
      }),
      run: ({ id, newSession, quickPlay }, job) =>
        api.launch({ instance: id, newSession, quickPlay }, job),
      invalidates: ({ id }) => [
        keys.instances.list(),
        keys.instances.detail(id),
        keys.accounts.all,
      ],
    }),
  /** `session` stops just that one; absent stops every session. */
  stopAny: () =>
    mutation<void, StopVars>({
      mutationKey: [...keys.instances.all, 'stop'],
      mutationFn: ({ id, session }) => api.stop(id, session),
      invalidates: ({ id }) => [
        keys.instances.list(),
        keys.instances.detail(id),
      ],
    }),
  /**
   * The first half of a restart: stops one session and settles only once the
   * daemon no longer lists it running, so the relaunch that follows is not
   * refused as a second session of the same game directory.
   */
  stopSettledAny: () =>
    mutation<void, Required<StopVars>>({
      mutationKey: [...keys.instances.all, 'stop', 'settled'],
      mutationFn: async ({ id, session }) => {
        await api.stop(id, session);
        await sessionExited(id, session);
      },
      invalidates: ({ id }) => [
        keys.instances.list(),
        keys.instances.detail(id),
      ],
    }),
  /** `memory` and `jvm-args` only. */
  setConfig: (id: string) =>
    mutation<void, ConfigEntry>({
      mutationKey: [...keys.instances.detail(id), 'config', 'set'],
      mutationFn: ({ key, value }) => api.config.set(id, key, value),
      invalidates: () => [keys.instances.config(id)],
    }),
  /** Instances take mods, resourcepacks, shaders, and datapacks. */
  content: entryContentFactories({
    kind: 'instance',
    api: api.content,
    contentKey: keys.instances.content,
    infoKey: keys.instances.info,
  }),
};

export function useInstances() {
  const iconFor = useEntryIconLookup();
  return useQuery({
    ...instanceQueries.list(),
    select: (instances) =>
      instances.map((instance) => ({
        ...instance,
        iconUrl: iconFor(instance.id),
      })),
  });
}

/** One instance, selected out of the list query (there is no status channel). */
export function useInstance(id: string) {
  const iconFor = useEntryIconLookup();
  return useQuery({
    ...instanceQueries.list(),
    select: (instances: InstanceInfo[]) => {
      const instance = instances.find((entry) => entry.id === id);
      return instance ? { ...instance, iconUrl: iconFor(instance.id) } : null;
    },
  });
}

/**
 * Follows the named session, or every session of the instance — including
 * sessions launched later, so following survives a stop and picks the next
 * launch up.
 */
export function useInstanceLogs(
  id: string,
  options: LogsOptions & { session?: string } = {},
): LogsResult {
  const query = useQuery({
    ...instanceQueries.logs(id, {
      session: options.session,
      tail: options.tail,
    }),
    staleTime: options.follow ? Number.POSITIVE_INFINITY : undefined,
  });
  const session = options.session;
  return useFollowedLogs(
    query,
    options.follow
      ? (processId) =>
          session
            ? processId === session
            : processId.startsWith(`instance-${id}_`)
      : null,
    options.limit,
  );
}
