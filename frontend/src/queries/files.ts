/**
 * `server.files.*` — a server's game directory. Every write sweeps the whole
 * `files` prefix: a move or an extract changes listings the caller never
 * named.
 */
import { queryOptions } from '@tanstack/react-query';
import { files as api } from '../api';
import { mutation } from './core';
import { keys } from './keys';

export const filesQueries = {
  list: (id: string, path: string) =>
    queryOptions({
      queryKey: keys.servers.fileList(id, path),
      queryFn: () => api.list(id, path),
    }),
  /** Never served stale: an editor must open on what is on disk now. */
  content: (id: string, path: string) =>
    queryOptions({
      queryKey: keys.servers.fileContent(id, path),
      queryFn: () => api.read(id, path),
      staleTime: 0,
      gcTime: 0,
      retry: false,
    }),
};

const write = <TVariables>(
  id: string,
  action: string,
  mutationFn: (variables: TVariables) => Promise<void>,
) =>
  mutation<void, TVariables>({
    mutationKey: [...keys.servers.files(id), action],
    mutationFn,
    invalidates: () => [keys.servers.files(id)],
  });

export const filesMutations = {
  write: (id: string) =>
    write<{ path: string; content: string }>(id, 'write', (v) =>
      api.write(id, v.path, v.content),
    ),
  mkdir: (id: string) =>
    write<string>(id, 'mkdir', (path) => api.mkdir(id, path)),
  move: (id: string) =>
    write<{ from: string; to: string }>(id, 'move', (v) =>
      api.move(id, v.from, v.to),
    ),
  copy: (id: string) =>
    write<{ from: string; to: string }>(id, 'copy', (v) =>
      api.copy(id, v.from, v.to),
    ),
  remove: (id: string) =>
    write<string[]>(id, 'remove', (paths) => api.remove(id, paths)),
  importFiles: (id: string) =>
    write<{ path: string; sources: string[] }>(id, 'import', (v) =>
      api.importFiles(id, v.path, v.sources),
    ),
  compress: (id: string) =>
    write<{ paths: string[]; to: string }>(id, 'compress', (v) =>
      api.compress(id, v.paths, v.to),
    ),
  extract: (id: string) =>
    write<{ from: string; to: string }>(id, 'extract', (v) =>
      api.extract(id, v.from, v.to),
    ),
};
