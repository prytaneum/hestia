import { MagnifyingGlassIcon, WarningCircleIcon } from '@phosphor-icons/react';
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useCallback, useMemo } from 'react';

import { type ContentKind, errorMessage } from '@/api';
import { useSearch } from '@/components/app-shell/search-context';
import { Empty } from '@/components/empty';
import { FilterMenu } from '@/components/filter-menu';
import { OfflineState } from '@/components/offline-state';
import { Page } from '@/components/page';
import { CardGridSkeleton } from '@/components/skeleton';
import {
  ContentCard,
  ResultGrid,
  sourceGroup,
  useSourceOptions,
} from '@/features/content/components';
import { mergeHits } from '@/features/content/lib';
import { TourButton } from '@/features/onboarding';
import { KindChips } from '@/features/shared/content/components';
import { contentKinds, kindInfo } from '@/features/shared/content/lib';
import { m } from '@/paraglide/messages.js';
import { contentQueries, isContentUrl } from '@/queries/content';
import { useOffline } from '@/queries/net';

const GRID = 'grid grid-cols-1 gap-3 xl:grid-cols-2';

export function BrowsePage({
  kind,
  source = '',
  onSourceChange,
}: {
  kind?: ContentKind;
  source?: string;
  onSourceChange?: (source: string) => void;
}) {
  const { query } = useSearch();
  const q = query.trim();
  const offline = useOffline();
  const sources = useSourceOptions(kind, source);
  // The default source is the absence of the param, not an empty one.
  const sourceParam = source ? { source } : {};

  // The kind is the route, so narrowing to one is navigation — which keeps a
  // browsed kind shareable and survives a reload.
  const navigate = useNavigate();
  const goToKind = (next?: ContentKind) =>
    next
      ? navigate({
          to: '/browse/$kind',
          params: { kind: kindInfo[next].slug },
          search: sourceParam,
        })
      : navigate({ to: '/browse', search: sourceParam });

  const url = isContentUrl(q) ? q : '';
  const link = useQuery(contentQueries.url(url));

  // A specific kind is one search; "All" fans out over every kind and merges,
  // since a source's search is scoped to a single project type.
  const kinds = kind ? [kind] : contentKinds;
  const search = useInfiniteQuery({
    ...contentQueries.searchPaged(kinds, q, sources.active),
    enabled: !url,
  });
  const { fetchNextPage, hasNextPage, isFetchingNextPage } = search;

  const hits = useMemo(
    () => mergeHits(search.data?.pages),
    [search.data?.pages],
  );

  const growFeed = useCallback(() => {
    fetchNextPage();
  }, [fetchNextPage]);

  return (
    <Page
      title={m['app.nav.browse']()}
      subtitle={m['content.browse.subtitle']()}
      search
      searchPlaceholder={m['app.search.content_or_link']()}
      actions={
        <>
          <TourButton id="browse" />
          <FilterMenu
            groups={[
              sourceGroup(sources.list, sources.active, (next) =>
                onSourceChange?.(next),
              ),
            ]}
          />
        </>
      }
      skeleton={<CardGridSkeleton grid={GRID} count={8} card="h-24" />}
    >
      <div className="flex h-full flex-col">
        <KindChips
          kinds={contentKinds}
          kind={kind}
          onKindChange={goToKind}
          className="mb-5 shrink-0"
        />
        <div className="min-h-0 flex-1">
          {offline ? (
            <OfflineState />
          ) : url ? (
            link.isPending ? (
              <CardGridSkeleton grid={GRID} count={1} card="h-24" />
            ) : link.data ? (
              <div className={GRID}>
                <ContentCard
                  project={link.data.project}
                  pinnedVersion={link.data.versionId || undefined}
                />
              </div>
            ) : (
              <Empty icon={WarningCircleIcon} tone="destructive">
                {errorMessage(link.error)}
              </Empty>
            )
          ) : search.isPending ? (
            <CardGridSkeleton grid={GRID} count={8} card="h-24" />
          ) : hits.length === 0 ? (
            <Empty icon={MagnifyingGlassIcon}>
              {m['content.browse.nothing_matches']()}
            </Empty>
          ) : (
            <ResultGrid
              hits={hits}
              hasMore={hasNextPage}
              loadingMore={isFetchingNextPage}
              onReachEnd={growFeed}
            />
          )}
        </div>
      </div>
    </Page>
  );
}
