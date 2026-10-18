/**
 * `instance.*` — the `instanceQueries`/`instanceMutations` factories, consumed
 * directly through useQuery/useMutation/useJobMutation, keyed by stable id like
 * the server factories. Only the behavior-bearing hooks stay named (below):
 * there is no `instance.status` channel, so `useInstance` selects the entry
 * from the list query. `update` is a job (a modpack moves with the version),
 * and so is `launch`, which materialises files and spawns the game.
 */

import { queryOptions, useQuery } from '@tanstack/react-query';
import { instanceSessionPrefix } from '@/lib/process-id';
import type {
  InstanceCreateParams,
  InstanceInfo,
  InstanceLaunchDone,
  InstanceServersWriteResult,
  InstanceUpdateDone,
  InstanceUpdateParams,
  QuickPlay,
  Replace,
} from '../api';
import * as entry from '../api/entry';
import * as api from '../api/instance';
import { mutation } from './core';
import { useEntryIconLookup } from './icons';
import { jobMutation } from './jobs';
import { entryTarget, keys } from './keys';
import { type LogsOptions, type LogsResult, useFollowedLogs } from './logs';

const target = (id: string) => entryTarget('instance', id);

/** A launch, and whether it may run alongside the sessions already up. */
interface LaunchVars {
  id: string;
  newSession?: boolean;
  quickPlay?: QuickPlay;
  replace?: Replace;
}

/** A stop aimed at one session of an instance, or at all of them. */
interface StopVars {
  id: string;
  session?: string;
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
      queryFn: () => entry.logs(target(id), options),
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
    jobMutation<
      InstanceUpdateDone,
      Omit<InstanceUpdateParams, 'instance' | 'id'>
    >({
      mutationKey: [...keys.instances.detail(id), 'update'],
      meta: (params) => ({
        kind: 'instance.update',
        label: `update to ${params.version}`,
        entry: { kind: 'instance', id },
      }),
      run: (params, job) => api.update({ ...params, instance: id }, job),
      invalidates: () => [
        keys.instances.list(),
        keys.instances.detail(id),
        keys.instances.info(id),
      ],
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
    jobMutation<InstanceLaunchDone, LaunchVars>({
      mutationKey: [...keys.instances.all, 'launch'],
      meta: ({ id }) => ({
        kind: 'instance.launch',
        label: 'launch',
        entry: { kind: 'instance', id },
      }),
      run: ({ id, newSession, quickPlay, replace }, job) =>
        api.launch({ instance: id, newSession, quickPlay, replace }, job),
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
      mutationFn: ({ id, session }) => entry.stop(target(id), session),
      invalidates: ({ id }) => [
        keys.instances.list(),
        keys.instances.detail(id),
      ],
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
            : processId.startsWith(instanceSessionPrefix(id))
      : null,
    options.limit,
  );
}
