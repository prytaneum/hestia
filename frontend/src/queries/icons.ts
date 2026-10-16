/** Custom entry icons — the `icons_*` shell commands as queries/mutations. */
import { queryOptions, useQuery } from '@tanstack/react-query';
import { useCallback } from 'react';
import * as api from '../api/icons';
import { mutation } from './core';
import { keys } from './keys';

export const iconQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.icons.list(),
      queryFn: () => api.list(),
    }),
  config: (entryId: string) =>
    queryOptions({
      queryKey: keys.icons.config(entryId),
      queryFn: () => api.config(entryId),
    }),
};

export const iconMutations = {
  set: () =>
    mutation<api.IconEntry, { entryId: string; sourcePath: string }>({
      mutationKey: [...keys.icons.all, 'set'],
      mutationFn: ({ entryId, sourcePath }) => api.set(entryId, sourcePath),
      invalidates: () => [keys.icons.all],
    }),
  generate: () =>
    mutation<
      api.IconEntry,
      { entryId: string; config: api.IconConfig; bytes: number[] }
    >({
      mutationKey: [...keys.icons.all, 'generate'],
      mutationFn: ({ entryId, config, bytes }) =>
        api.generate(entryId, config, bytes),
      invalidates: () => [keys.icons.all],
    }),
};

/**
 * A stable resolver from entry id to its icon URL — the join the server and
 * instance query hooks use so list rows and detail views carry `iconUrl`
 * directly instead of each call site re-joining the icons map.
 */
export function useEntryIconLookup(): (id: string) => string | undefined {
  const map = useQuery(iconQueries.list()).data;
  return useCallback(
    (id) => {
      const entry = map?.[id];
      return entry ? api.iconUrl(entry) : undefined;
    },
    [map],
  );
}
