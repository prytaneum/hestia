/** `config.*` — query/mutation factories, consumed through useQuery/useMutation. */
import { queryOptions, useQuery } from '@tanstack/react-query';
import * as api from '../api/config';
import { mutation } from './core';
import { keys } from './keys';

export const configQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.config.list(),
      queryFn: () => api.list(),
    }),
  value: (key: string) =>
    queryOptions({
      queryKey: keys.config.value(key),
      queryFn: () => api.get(key),
    }),
};

/**
 * The launcher-wide fallbacks an entry inherits when it sets no override. The
 * settings document is an opaque tree on the wire, so this is the one place
 * that knows its shape.
 */
export interface LauncherDefaults {
  memory?: string;
  'jvm-args'?: string;
}

export function launcherDefaults(
  config: Record<string, unknown> | undefined,
): LauncherDefaults {
  return (
    (config as { defaults?: LauncherDefaults } | undefined)?.defaults ?? {}
  );
}

export const configMutations = {
  set: () =>
    mutation<void, { key: string; value: unknown }>({
      mutationKey: [...keys.config.all, 'set'],
      mutationFn: ({ key, value }) => api.set(key, value),
      // A content source's API key is a setting, and setting it changes which
      // sources answer — so the browse cache goes with the config cache.
      invalidates: () => [keys.config.all, keys.content.all],
    }),
};

/**
 * Whether an instance may run several sessions at once
 * (`instance.multi-session`). Unresolved reads as off, so the concurrent-launch
 * affordances never offer what the daemon would refuse.
 */
export function useMultiSession(): boolean {
  const { data } = useQuery(configQueries.value(MULTI_SESSION_KEY));
  return data === true;
}

/**
 * Whether per-session UI applies: multi-session is on, or sessions launched
 * while it was on are still running and each needs its own stop.
 */
export function useSessionsShown(running: number): boolean {
  return useMultiSession() || running > 1;
}

const MULTI_SESSION_KEY = 'instance.multi-session';
