import {
  ArrowClockwiseIcon,
  MagnifyingGlassIcon,
  PuzzlePieceIcon,
} from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import { errorHint, errorMessage } from '@/api';
import { Empty } from '@/components/empty';
import { FilterMenu } from '@/components/filter-menu';
import { CardGridSkeleton } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { WarningNotice } from '@/components/warning-notice';
import { m } from '@/paraglide/messages.js';
import { pluginQueries } from '@/queries/plugin';

import { CatalogueCard, PLUGIN_GRID } from './card';
import type { PluginInstall } from './install';
import { matchesQuery } from './state';

const ALL = 'all';

/**
 * Everything the added repositories offer, one card per plugin and signing
 * key, narrowed by the page's search. Installing goes through the same
 * inspect, native approval and install as any other package.
 */
export function PluginBrowse({
  query,
  install,
  onRepositories,
}: {
  query: string;
  install: PluginInstall;
  onRepositories: () => void;
}) {
  const catalogue = useQuery(pluginQueries.catalogue());
  const [repo, setRepo] = useState(ALL);

  if (catalogue.isPending)
    return <CardGridSkeleton grid={PLUGIN_GRID} count={6} card="h-24" />;
  if (catalogue.isError)
    return (
      <Empty
        tone="destructive"
        description={errorHint(catalogue.error) || undefined}
      >
        {errorMessage(catalogue.error)}
      </Empty>
    );

  const { repositories, plugins, warnings } = catalogue.data;
  if (repositories.length === 0)
    return (
      <Empty
        icon={PuzzlePieceIcon}
        action={
          <Button variant="outline" size="sm" onClick={onRepositories}>
            {m['plugins.manage.add_repository']()}
          </Button>
        }
      >
        {m['settings.plugins.browse.none']()}
      </Empty>
    );

  const shown = plugins.filter(
    (plugin) =>
      (repo === ALL || plugin.listedBy.some((lister) => lister.url === repo)) &&
      matchesQuery(query, [plugin.id, plugin.name, plugin.description]),
  );

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-end gap-2">
        <FilterMenu
          groups={[
            repositories.length > 1
              ? {
                  label: m['settings.plugins.browse.repository'](),
                  value: repo,
                  neutral: ALL,
                  options: [
                    { value: ALL, label: m['app.label.all']() },
                    ...repositories.map((r) => ({
                      value: r.url,
                      label: r.name || r.label,
                    })),
                  ],
                  onChange: setRepo,
                }
              : undefined,
          ]}
        />
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={m['app.action.refresh']()}
          disabled={catalogue.isFetching}
          onClick={() => void catalogue.refetch()}
        >
          <ArrowClockwiseIcon className="size-4" />
        </Button>
      </div>

      <WarningNotice warnings={warnings} />

      {shown.length === 0 ? (
        <Empty icon={MagnifyingGlassIcon}>
          {m['settings.plugins.browse.empty']()}
        </Empty>
      ) : (
        <div className={PLUGIN_GRID}>
          {shown.map((plugin) => (
            <CatalogueCard
              key={`${plugin.keyId}/${plugin.id}`}
              plugin={plugin}
              busy={install.busy}
              onInstall={() =>
                void install.start({
                  repo: plugin.listedBy[0].url,
                  plugin: plugin.id,
                })
              }
            />
          ))}
        </div>
      )}
    </div>
  );
}
