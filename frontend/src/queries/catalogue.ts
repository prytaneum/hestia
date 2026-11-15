/** What a server or an instance can be created as, keyed under its kind's tree. */
import { queryOptions, useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import type { Edition, EntryKind } from '../api';
import * as api from '../api/entry';
import { providerNamer } from '../lib/provider-name';
import { contentQueries } from './content';
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

/** The one place the desktop shortens provider ids: flavors of both kinds and content sources. */
export function useProviderNamer(): (id: string) => string {
  const servers = useQuery(catalogueQueries.flavors('server')).data?.editions;
  const instances = useQuery(catalogueQueries.flavors('instance')).data
    ?.editions;
  const sources = useQuery(contentQueries.sources()).data;
  return useMemo(
    () =>
      providerNamer(
        [
          ...[...(servers ?? []), ...(instances ?? [])].flatMap(
            (catalogue) => catalogue.flavors,
          ),
          ...(sources ?? []),
        ].map((provider) => provider.id),
      ),
    [servers, instances, sources],
  );
}
