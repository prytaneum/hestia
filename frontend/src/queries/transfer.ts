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
import type { ExportDone, ImportDone } from '../api';
import type { ExportInput } from '../api/transfer';
import * as api from '../api/transfer';
import { jobMutation } from './jobs';
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
  export: (id: string, name: string) =>
    jobMutation<ExportDone, ExportInput>({
      mutationKey: [...keys.instances.detail(id), 'export'],
      meta: (input) => ({
        kind: 'instance.export',
        label: `export ${name} (${input.format})`,
        entry: { kind: 'instance', id },
      }),
      run: (input, job) => api.exportInstance(id, input, job),
      invalidates: () => [[FOOTPRINT]],
    }),
  import: () =>
    jobMutation<ImportDone, { path: string; name?: string }>({
      mutationKey: [...keys.instances.all, 'import'],
      meta: ({ path }) => ({
        kind: 'instance.import',
        label: `import ${fileName(path)}`,
      }),
      run: ({ path, name }, job) => api.importInstance(path, name ?? '', job),
      invalidates: () => [keys.instances.list()],
    }),
};

function fileName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}
