/**
 * `modpack.*` — the pack an entry runs, and the jobs that put one there.
 *
 * Installing is deliberately not keyed to an entry: the common case *creates*
 * the entry, so there is no id to key by until the job's done event names one.
 * That is why the install mutations invalidate the whole list rather than one
 * entry's prefix.
 */
import { queryOptions } from '@tanstack/react-query';
import { logger } from '@/lib/log';
import { m } from '@/paraglide/messages.js';
import type {
  EntryKind,
  InstalledModpack,
  ModpackDone,
  ModpackFilePreview,
  ModpackRemoveResult,
  ModpackTarget,
  ModpackUpdate,
} from '../api';
import * as icons from '../api/icons';
import type { PackRef } from '../api/modpack';
import * as api from '../api/modpack';
import { mutation } from './core';
import { type JobOutcome, jobMutation } from './jobs';
import { entryScope, entryTarget, keys } from './keys';

function packOutcome(title: string, done: ModpackDone): JobOutcome {
  return {
    title,
    failures: {
      failed: done.failures.length,
      total: done.pack.files.length + done.failures.length,
    },
    warnings: done.warnings,
  };
}

const log = logger('modpack');

/** What an install job needs: the pack, where it goes, and a server's extras. */
interface InstallInput {
  pack: PackRef;
  target: ModpackTarget;
  eula?: boolean;
  port?: number;
}

type UpdateInput = api.PackMove;

/** A pack rewrites the pool and the entry's own record, so both are swept. */
function invalidates(kind: EntryKind, id: string) {
  const scope = entryScope(kind);
  return [scope.detail(id), scope.all];
}

export const modpackQueries = {
  status: (kind: EntryKind, id: string) =>
    queryOptions({
      queryKey: entryScope(kind).modpack(id),
      queryFn: (): Promise<InstalledModpack | null> =>
        api.status(entryTarget(kind, id)),
    }),
  /** Asked about one picked file, so the answer is never reused. */
  updatePreview: (kind: EntryKind, id: string, path: string) =>
    queryOptions({
      queryKey: [...entryScope(kind).modpack(id), 'preview', path],
      queryFn: (): Promise<ModpackFilePreview> =>
        api.previewUpdate(entryTarget(kind, id), path),
      staleTime: 0,
      gcTime: 0,
    }),
  /** A catalogue round trip, so it is not refetched on every mount. */
  updateCheck: (
    kind: EntryKind,
    id: string,
    enabled = true,
    gameVersion = '',
  ) =>
    queryOptions({
      queryKey: [...entryScope(kind).modpack(id), 'check', gameVersion],
      queryFn: (): Promise<ModpackUpdate | null> =>
        api.checkUpdate(entryTarget(kind, id), gameVersion),
      enabled,
      staleTime: 10 * 60_000,
      retry: false,
    }),
};

export const modpackMutations = {
  /** Creates its entry unless the target names an existing one. */
  install: (kind: EntryKind) =>
    jobMutation<ModpackDone, InstallInput>({
      mutationKey: ['modpack', kind, 'install'],
      meta: () => ({ channel: `${kind}.modpack.install` }),
      run: async ({ pack, target, eula, port }, job) => {
        const done =
          kind === 'server'
            ? await api.installServer(pack, target, { eula, port }, job)
            : await api.installInstance(pack, target, job);
        if (target.mode === 'create') await inheritPackIcon(done);
        return done;
      },
      invalidates: () => [keys.servers.all, keys.instances.all, keys.icons.all],
      notify: {
        done: (done) =>
          packOutcome(
            m['content.modpack.install_into']({ name: done.entryName }),
            done,
          ),
      },
    }),
  update: (kind: EntryKind, id: string) =>
    jobMutation<ModpackDone, UpdateInput>({
      mutationKey: [...entryScope(kind).modpack(id), 'update'],
      meta: () => ({
        channel: 'modpack.update',
        subject: { kind, id },
      }),
      run: (to, job) => api.update(entryTarget(kind, id), to, job),
      invalidates: () => invalidates(kind, id),
      notify: {
        done: (done) =>
          packOutcome(m['app.toast.updated']({ name: done.entryName }), done),
      },
    }),
  remove: (kind: EntryKind, id: string) =>
    mutation<ModpackRemoveResult, { keepContent: boolean }>({
      mutationKey: [...entryScope(kind).modpack(id), 'remove'],
      mutationFn: ({ keepContent }) =>
        api.remove(entryTarget(kind, id), keepContent),
      invalidates: () => invalidates(kind, id),
    }),
};

async function inheritPackIcon({ entry, pack }: ModpackDone) {
  if (!pack.iconUrl) return;
  try {
    await icons.fetch(entry, pack.iconUrl);
  } catch (error) {
    log.warn({ entry, error }, 'modpack icon not inherited');
  }
}
