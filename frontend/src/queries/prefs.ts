import { queryOptions, useMutation, useQuery } from '@tanstack/react-query';
import * as api from '../api/prefs';
import { queryClient } from './client';
import { mutation } from './core';
import { keys } from './keys';

export const prefsQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.prefs.list(),
      queryFn: () => api.list(),
    }),
};

// Writes are exact transforms, applied to the cache up front so dependent UI
// (pin toggles, drag reorders) never snaps back while the file write settles.
function optimisticPrefs(
  update: (prefs: Record<string, unknown>) => Record<string, unknown>,
): (() => void) | undefined {
  const key = keys.prefs.list();
  void queryClient.cancelQueries({ queryKey: key });
  const previous = queryClient.getQueryData<Record<string, unknown>>(key);
  if (!previous) return undefined;
  queryClient.setQueryData(key, update(previous));
  return () => queryClient.setQueryData(key, previous);
}

const prefsMutations = {
  set: () =>
    mutation<void, { key: string; value: unknown }>({
      mutationKey: [...keys.prefs.all, 'set'],
      mutationFn: ({ key, value }) => api.set(key, value),
      optimistic: ({ key, value }) =>
        optimisticPrefs((prefs) => ({ ...prefs, [key]: value })),
      invalidates: () => [keys.prefs.all],
    }),
  remove: () =>
    mutation<void, string>({
      mutationKey: [...keys.prefs.all, 'remove'],
      mutationFn: (key) => api.remove(key),
      optimistic: (key) =>
        optimisticPrefs((prefs) => {
          const { [key]: _removed, ...rest } = prefs;
          return rest;
        }),
      invalidates: () => [keys.prefs.all],
    }),
};

export function usePrefs() {
  const query = useQuery(prefsQueries.list());
  const setMutation = useMutation(prefsMutations.set());
  const removeMutation = useMutation(prefsMutations.remove());

  const prefs = query.data ?? {};

  return {
    prefs,
    ready: !query.isPending,
    get: <T>(key: string, fallback: T): T =>
      key in prefs ? (prefs[key] as T) : fallback,
    set: (key: string, value: unknown) => setMutation.mutate({ key, value }),
    remove: (key: string) => removeMutation.mutate(key),
  };
}
