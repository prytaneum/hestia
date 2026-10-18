/**
 * The verbs a server and an instance share, the side chosen by the `kind`
 * argument. Keys stay per entry and nest under its stable id.
 */
import { queryOptions } from '@tanstack/react-query';
import type { ConfigEntry, EntryKind, EntryRenamed } from '../api';
import * as api from '../api/entry';
import { mutation } from './core';
import { entryScope, entryTarget } from './keys';

export const entryQueries = {
  config: (kind: EntryKind, id: string) =>
    queryOptions({
      queryKey: entryScope(kind).config(id),
      queryFn: () => api.config.list(entryTarget(kind, id)),
    }),
};

export const entryMutations = {
  rename: (kind: EntryKind, id: string) => {
    const scope = entryScope(kind);
    return mutation<EntryRenamed, string>({
      mutationKey: [...scope.detail(id), 'rename'],
      mutationFn: (name) => api.rename(entryTarget(kind, id), name),
      invalidates: () => [scope.list(), scope.detail(id), scope.info(id)],
    });
  },
  remove: (kind: EntryKind, id: string) => {
    const scope = entryScope(kind);
    return mutation({
      mutationKey: [...scope.detail(id), 'remove'],
      mutationFn: () => api.remove(entryTarget(kind, id)),
      invalidates: () => [scope.list()],
    });
  },
  /** `session` stops just that one of an instance's; absent stops them all. */
  stop: (kind: EntryKind, id: string) => {
    const scope = entryScope(kind);
    return mutation<void, { session?: string }>({
      mutationKey: [...scope.detail(id), 'stop'],
      mutationFn: ({ session }) => api.stop(entryTarget(kind, id), session),
      invalidates: () => [scope.list(), scope.detail(id)],
    });
  },
  /** `memory` and `jvm-args`, plus any `server.properties` key on a server. */
  setConfig: (kind: EntryKind, id: string) => {
    const scope = entryScope(kind);
    return mutation<void, ConfigEntry>({
      mutationKey: [...scope.detail(id), 'config', 'set'],
      mutationFn: ({ key, value }) =>
        api.config.set(entryTarget(kind, id), key, value),
      invalidates: () => [scope.config(id)],
    });
  },
};
