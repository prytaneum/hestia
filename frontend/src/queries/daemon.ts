/** `daemon.*` — queries/mutations plus their 1:1 hooks. */
import { queryOptions, useMutation, useQuery } from '@tanstack/react-query';
import { useEffect, useReducer } from 'react';
import { uptime as formatUptime } from '@/lib/format';
import type { DaemonStatusResult } from '../api';
import * as api from '../api/daemon';
import { useConnection } from './connection';
import { mutation, optimisticUpdate } from './core';
import { keys } from './keys';

const daemonQueries = {
  status: () =>
    queryOptions({
      queryKey: keys.daemon.status(),
      queryFn: () => api.status(),
    }),
};

const daemonMutations = {
  /** Spawn a stopped daemon; the settle refetch fills in its live status. */
  start: () =>
    mutation<DaemonStatusResult, void>({
      mutationKey: [...keys.daemon.all, 'start'],
      mutationFn: () => api.start(),
      invalidates: () => [keys.daemon.all],
    }),
  /** Stop then respawn; optimistically reset the ticking uptime to zero. */
  restart: () =>
    mutation<DaemonStatusResult, void>({
      mutationKey: [...keys.daemon.all, 'restart'],
      mutationFn: () => api.restart(),
      optimistic: () =>
        optimisticUpdate<DaemonStatusResult>(
          keys.daemon.status(),
          (status) => ({
            ...status,
            uptimeSeconds: 0,
          }),
        ),
      invalidates: () => [keys.daemon.all],
    }),
};

/**
 * The uptime as a live, optimistic label: it ticks up every second from the
 * fetched `uptimeSeconds` anchored to when that read resolved, so it counts
 * on its own without refetching and resets the moment a restart writes a fresh
 * status into the cache.
 */
function useLiveUptime(
  status: DaemonStatusResult | undefined,
  updatedAt: number,
): string | null {
  const [, tick] = useReducer((n: number) => n + 1, 0);
  useEffect(() => {
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, []);
  if (!status) return null;
  const anchorMs = updatedAt - status.uptimeSeconds * 1000;
  const seconds = Math.max(0, (Date.now() - anchorMs) / 1000);
  return formatUptime(seconds);
}

/**
 * The daemon-management panel's one hook: connection state, live status, and
 * the start/restart actions. Status is read only while connected, so polling
 * never auto-spawns a daemon the user deliberately stopped.
 */
export function useDaemon() {
  const connected = useConnection() === 'connected';
  const status = useQuery({ ...daemonQueries.status(), enabled: connected });
  const uptime = useLiveUptime(status.data, status.dataUpdatedAt);
  const start = useMutation(daemonMutations.start());
  const restart = useMutation(daemonMutations.restart());
  return {
    connected,
    status: status.data,
    uptime,
    start,
    restart,
    busy: start.isPending || restart.isPending,
  };
}
