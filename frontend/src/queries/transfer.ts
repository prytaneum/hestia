/**
 * `instance.export` / `instance.import` — the transfer factories.
 *
 * Both writes are jobs, so they register in the global job store and keep
 * running when the dialog that started them is closed. An import invalidates
 * the instance list (it creates one); an export invalidates nothing but the
 * instance's own footprint, since the archive lands outside the data home as
 * often as not.
 */
import { queryOptions } from '@tanstack/react-query';
import { bytes } from '@/lib/format';
import { parentOf } from '@/lib/path';
import { m } from '@/paraglide/messages.js';
import { type ExportDone, type ImportDone, system } from '../api';
import type { ExportInput } from '../api/transfer';
import * as api from '../api/transfer';
import { jobMutation, subjectName } from './jobs';
import { FOOTPRINT, keys } from './keys';

export const transferQueries = {
  /**
   * The tree an export's exclusions are picked out of. Always fetched fresh:
   * it is a directory walk whose whole value is being current, and it is only
   * read while the export dialog is open.
   */
  contents: (id: string) =>
    queryOptions({
      queryKey: keys.instances.exportContents(id),
      queryFn: () => api.contents(id),
      staleTime: 0,
      gcTime: 0,
    }),
  /** What an archive says it is. Keyed by path — the file is the identity. */
  archive: (path: string) =>
    queryOptions({
      queryKey: keys.transfer.archive(path),
      queryFn: () => api.inspect(path),
      enabled: path.length > 0,
      retry: false,
      staleTime: 0,
      gcTime: 0,
    }),
};

export const transferMutations = {
  export: (id: string) =>
    jobMutation<ExportDone, ExportInput>({
      mutationKey: [...keys.instances.detail(id), 'export'],
      meta: () => ({
        channel: 'instance.export',
        subject: { kind: 'instance', id },
      }),
      run: (input, job) => api.exportInstance(id, input, job),
      invalidates: () => [[FOOTPRINT]],
      notify: {
        done: (done) => ({
          title: m['instance.export.done']({
            name: subjectName({ kind: 'instance', id }),
          }),
          description: m['instance.export.summary']({
            files: done.files,
            size: bytes(done.sizeBytes),
          }),
          action: {
            label: m['app.action.open_folder'](),
            onClick: () => void system.openPath(parentOf(done.path)),
          },
          warnings: done.warnings,
        }),
      },
    }),
  import: () =>
    jobMutation<ImportDone, { path: string; name?: string }>({
      mutationKey: [...keys.instances.all, 'import'],
      meta: () => ({ channel: 'instance.import' }),
      run: ({ path, name }, job) => api.importInstance(path, name ?? '', job),
      invalidates: () => [keys.instances.list()],
      notify: {
        done: (done) => ({
          title: m['instance.import.done']({ name: done.instance.name }),
          failures: { failed: done.failures.length },
          warnings: done.warnings,
        }),
      },
    }),
};
