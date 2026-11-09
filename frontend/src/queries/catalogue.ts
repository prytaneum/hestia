/** What a server or an instance can be created as, keyed under its kind's tree. */
import { queryOptions } from '@tanstack/react-query';
import type { Edition, EntryKind } from '../api';
import * as api from '../api/entry';
import { CATALOG_STALE_MS } from './core';
import { entryScope } from './keys';

export const catalogueQueries = {
  flavors: (kind: EntryKind) =>
    queryOptions({
      queryKey: entryScope(kind).flavors(),
      queryFn: () => api.flavors(kind),
      staleTime: CATALOG_STALE_MS,
    }),
  versions: (kind: EntryKind, edition: Edition, flavor: string) =>
    queryOptions({
      queryKey: entryScope(kind).versions(edition, flavor),
      queryFn: () => api.versions(kind, edition, flavor),
      staleTime: CATALOG_STALE_MS,
    }),
  loaders: (
    kind: EntryKind,
    edition: Edition,
    flavor: string,
    version: string,
  ) =>
    queryOptions({
      queryKey: entryScope(kind).loaders(edition, flavor, version),
      queryFn: () => api.loaders(kind, edition, flavor, version),
      staleTime: CATALOG_STALE_MS,
    }),
};
