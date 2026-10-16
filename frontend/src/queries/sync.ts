/**
 * `sync.*` — the catalogue of shared settings and each instance's standing.
 */
import { queryOptions } from '@tanstack/react-query';
import type {
  InstanceSyncStatus,
  SyncConfig,
  SyncOption,
  SyncUnit,
} from '../api';
import * as api from '../api/sync';
import { mutation } from './core';
import { keys } from './keys';

export const syncQueries = {
  config: () =>
    queryOptions({
      queryKey: keys.sync.config(),
      queryFn: () => api.get(),
    }),
  status: () =>
    queryOptions({
      queryKey: keys.sync.status(),
      queryFn: () => api.status(),
    }),
  options: () =>
    queryOptions({
      queryKey: keys.sync.options(),
      queryFn: () => api.options(),
    }),
  sources: (unit: SyncUnit) =>
    queryOptions({
      queryKey: keys.sync.sources(unit),
      queryFn: () => api.sources(unit),
    }),
};

export const syncMutations = {
  /** Seeded from the named instance; empty picks the only candidate. */
  enable: () =>
    mutation<SyncConfig, { unit: SyncUnit; source?: string }>({
      mutationKey: [...keys.sync.all, 'enable'],
      mutationFn: ({ unit, source }) => api.enable(unit, source),
      invalidates: () => [keys.sync.all],
    }),
  disable: () =>
    mutation<SyncConfig, SyncUnit>({
      mutationKey: [...keys.sync.all, 'disable'],
      mutationFn: (unit) => api.disable(unit),
      invalidates: () => [keys.sync.all],
    }),
  setUnsynced: () =>
    mutation<SyncConfig, string[]>({
      mutationKey: [...keys.sync.all, 'unsynced'],
      mutationFn: (unsynced) => api.setUnsynced(unsynced),
      invalidates: () => [keys.sync.all],
    }),
  setOption: () =>
    mutation<SyncOption[], { key: string; value: string }>({
      mutationKey: [...keys.sync.all, 'option'],
      mutationFn: ({ key, value }) => api.setOption(key, value),
      invalidates: () => [keys.sync.all],
    }),
  setInstanceUnit: (id: string) =>
    mutation<InstanceSyncStatus, { unit: SyncUnit; shared: boolean | null }>({
      mutationKey: [...keys.instances.detail(id), 'sync', 'unit'],
      mutationFn: ({ unit, shared }) => api.setInstanceUnit(id, unit, shared),
      invalidates: () => [keys.sync.all, keys.instances.detail(id)],
    }),
  setInstanceUnsynced: (id: string) =>
    mutation<InstanceSyncStatus, string[]>({
      mutationKey: [...keys.instances.detail(id), 'sync', 'keys'],
      mutationFn: (unsynced) => api.setInstanceUnsynced(id, unsynced),
      invalidates: () => [keys.sync.all, keys.instances.detail(id)],
    }),
};
