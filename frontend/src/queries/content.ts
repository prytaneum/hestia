/**
 * `content.*` browse — discovery on a source platform (queries only;
 * installing into an entry lives on the server/instance hooks). An empty
 * `source` selects the default source; `sources()` answers only the ones that
 * can serve, so a platform whose API key is unset never appears.
 */
import {
  infiniteQueryOptions,
  keepPreviousData,
  queryOptions,
} from '@tanstack/react-query';
import type {
  ContentKind,
  SearchQuery,
  SearchResult,
  VersionQuery,
} from '../api';
import * as api from '../api/content';
import { keys } from './keys';

const BROWSE_STALE_MS = 60_000;

/** Hits fetched per page when paging the browse grid. */
const SEARCH_PAGE = 20;

/** Shape only; which sources are recognised is the daemon's answer. */
export const isContentUrl = (text: string): boolean =>
  /^https?:\/\/\S+$/i.test(text.trim());

/** Per-kind browse offset; `null` once that kind is exhausted. */
type PagedOffsets = Partial<Record<ContentKind, number | null>>;

export const contentQueries = {
  sources: () =>
    queryOptions({
      queryKey: keys.content.sources(),
      queryFn: () => api.sources(),
      staleTime: Number.POSITIVE_INFINITY,
    }),
  search: (query: Partial<SearchQuery>) =>
    queryOptions({
      queryKey: keys.content.search(query),
      queryFn: () => api.search(query),
      staleTime: BROWSE_STALE_MS,
      // A single observer transitions keys cleanly, so keep the current hits on
      // screen while a new query loads (search-as-you-type never blanks).
      placeholderData: keepPreviousData,
    }),
  // "All" fans out over every kind, each with its own offset (`null` once
  // exhausted) so a short kind stops being queried while a long one keeps
  // paging; the grid merges the per-kind hits.
  searchPaged: (kinds: ContentKind[], query: string, source = '') =>
    infiniteQueryOptions({
      queryKey: keys.content.searchPaged(kinds, query, source),
      queryFn: ({ pageParam }) =>
        Promise.all(
          kinds.map((kind) => {
            const offset = pageParam[kind];
            if (offset == null)
              return Promise.resolve<SearchResult>({
                hits: [],
                offset: 0,
                limit: SEARCH_PAGE,
                total: 0,
              });
            return api.search({
              kind,
              query,
              source,
              limit: SEARCH_PAGE,
              offset,
            });
          }),
        ),
      initialPageParam: Object.fromEntries(
        kinds.map((kind) => [kind, 0]),
      ) as PagedOffsets,
      getNextPageParam: (lastPage, _pages, lastParam) => {
        const next: PagedOffsets = {};
        let more = false;
        kinds.forEach((kind, index) => {
          const prev = lastParam[kind];
          if (prev == null) {
            next[kind] = null;
            return;
          }
          const result = lastPage[index];
          const exhausted = result.offset + result.hits.length >= result.total;
          next[kind] = exhausted ? null : prev + SEARCH_PAGE;
          if (!exhausted) more = true;
        });
        return more ? next : undefined;
      },
      staleTime: BROWSE_STALE_MS,
      placeholderData: keepPreviousData,
    }),
  project: (project: string, source = '') =>
    queryOptions({
      queryKey: keys.content.project(source, project),
      queryFn: () => api.project(project, source),
      enabled: project.length > 0,
      staleTime: BROWSE_STALE_MS,
    }),
  versions: (query: Partial<VersionQuery>) =>
    queryOptions({
      queryKey: keys.content.versions(query),
      queryFn: () => api.versions(query),
      enabled: (query.project ?? '').length > 0,
      staleTime: BROWSE_STALE_MS,
    }),
  /** Enabled only for URL-shaped input, so a search box can hand it anything. */
  url: (url: string) =>
    queryOptions({
      queryKey: keys.content.url(url),
      queryFn: () => api.resolveUrl(url),
      enabled: isContentUrl(url),
      retry: false,
      staleTime: BROWSE_STALE_MS,
    }),
};
