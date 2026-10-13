/**
 * `*.modpack.*` — the pack an entry runs, and the jobs that put one there.
 *
 * Installing is deliberately not keyed to an entry: the common case *creates*
 * the entry, so there is no id to key by until the job's done event names one.
 * That is why the install mutations invalidate the whole list rather than one
 * entry's prefix.
 */
import { queryOptions, useMutation, useQuery } from '@tanstack/react-query';
import { logger } from '@/lib/log';
import type {
  InstalledModpack,
  ModpackDoneEvent,
  ModpackFilePreview,
  ModpackRemoveResult,
  ModpackTarget,
  ModpackUpdate,
} from '../api';
import * as icons from '../api/icons';
import type { PackRef } from '../api/modpack';
import * as api from '../api/modpack';
import { mutation } from './core';
import { type JobEntryKind, jobMutation, useJobMutation } from './jobs';
import { keys } from './keys';

const log = logger('modpack');

/** What an install job needs: the pack, where it goes, and a server's extras. */
export interface InstallInput {
  pack: PackRef;
  target: ModpackTarget;
  eula?: boolean;
  port?: number;
}

export type UpdateInput = api.PackMove;

function entryKeys(kind: JobEntryKind) {
  return kind === 'server' ? keys.servers : keys.instances;
}

/** A pack rewrites the pool and the entry's own record, so both are swept. */
function invalidates(kind: JobEntryKind, id: string) {
  const scope = entryKeys(kind);
  return [scope.detail(id), scope.all];
}

export const modpackQueries = {
  status: (kind: JobEntryKind, id: string) =>
    queryOptions({
      queryKey: entryKeys(kind).modpack(id),
      queryFn: (): Promise<InstalledModpack | null> =>
        kind === 'server' ? api.serverStatus(id) : api.instanceStatus(id),
    }),
  /** Asked about one picked file, so the answer is never reused. */
  updatePreview: (kind: JobEntryKind, id: string, path: string) =>
    queryOptions({
      queryKey: [...entryKeys(kind).modpack(id), 'preview', path],
      queryFn: (): Promise<ModpackFilePreview> =>
        kind === 'server'
          ? api.previewServerUpdate(id, path)
          : api.previewInstanceUpdate(id, path),
      staleTime: 0,
      gcTime: 0,
    }),
  /** A catalogue round trip, so it is not refetched on every mount. */
  updateCheck: (kind: JobEntryKind, id: string, enabled = true) =>
    queryOptions({
      queryKey: [...entryKeys(kind).modpack(id), 'check'],
      queryFn: (): Promise<ModpackUpdate | null> =>
        kind === 'server'
          ? api.serverCheckUpdate(id)
          : api.instanceCheckUpdate(id),
      enabled,
      staleTime: 10 * 60_000,
      retry: false,
    }),
};

export const modpackMutations = {
  /** Creates its entry unless the target names an existing one. */
  install: (kind: JobEntryKind) =>
    jobMutation<ModpackDoneEvent, InstallInput>({
      mutationKey: ['modpack', kind, 'install'],
      meta: () => ({ kind: 'modpack.install', label: 'install modpack' }),
      run: async ({ pack, target, eula, port }, job) => {
        const done =
          kind === 'server'
            ? await api.installServer(pack, target, { eula, port }, job)
            : await api.installInstance(pack, target, job);
        if (target.mode === 'create') await inheritPackIcon(done);
        return done;
      },
      invalidates: () => [keys.servers.all, keys.instances.all, keys.icons.all],
    }),
  update: (kind: JobEntryKind, id: string) =>
    jobMutation<ModpackDoneEvent, UpdateInput>({
      mutationKey: [...entryKeys(kind).modpack(id), 'update'],
      meta: () => ({
        kind: 'modpack.update',
        label: 'update modpack',
        entry: { kind, id },
      }),
      run: (to, job) =>
        kind === 'server'
          ? api.updateServer(id, to, job)
          : api.updateInstance(id, to, job),
      invalidates: () => invalidates(kind, id),
    }),
  remove: (kind: JobEntryKind, id: string) =>
    mutation<ModpackRemoveResult, { keepContent: boolean }>({
      mutationKey: [...entryKeys(kind).modpack(id), 'remove'],
      mutationFn: ({ keepContent }) =>
        kind === 'server'
          ? api.removeServer(id, keepContent)
          : api.removeInstance(id, keepContent),
      invalidates: () => invalidates(kind, id),
    }),
};

async function inheritPackIcon({ entry, pack }: ModpackDoneEvent) {
  if (!pack.iconUrl) return;
  try {
    await icons.fetch(entry, pack.iconUrl);
  } catch (error) {
    log.warn({ entry, error }, 'modpack icon not inherited');
  }
}

export function useModpack(kind: JobEntryKind, id: string) {
  return useQuery(modpackQueries.status(kind, id));
}

export function useModpackUpdateCheck(
  kind: JobEntryKind,
  id: string,
  enabled = true,
) {
  return useQuery(modpackQueries.updateCheck(kind, id, enabled));
}

export function useInstallModpack(kind: JobEntryKind) {
  return useJobMutation(modpackMutations.install(kind));
}

export function useUpdateModpack(kind: JobEntryKind, id: string) {
  return useJobMutation(modpackMutations.update(kind, id));
}

export function useRemoveModpack(kind: JobEntryKind, id: string) {
  return useMutation(modpackMutations.remove(kind, id));
}
