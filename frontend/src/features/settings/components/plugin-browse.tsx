import { ArrowClockwiseIcon, PuzzlePieceIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useMemo, useState } from 'react';

import { type Availability, type CataloguePlugin, errorMessage } from '@/api';
import { FilterMenu } from '@/components/filter-menu';
import { SearchInput } from '@/components/search-input';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Thumbnail } from '@/components/ui/thumbnail';
import { WarningNotice } from '@/components/warning-notice';
import { m } from '@/paraglide/messages.js';
import { pluginQueries } from '@/queries/plugin';

import { InstallReview, usePluginInstall } from './plugin-install';

const ALL = 'all';

const AVAILABILITY_LABEL: Record<Availability, () => string> = {
  prebuilt: m['domain.plugin.availability.prebuilt'],
  unreleased: m['domain.plugin.availability.unreleased'],
  incompatible: m['domain.plugin.availability.incompatible'],
};

function availabilityHint(plugin: CataloguePlugin): string | undefined {
  const reason =
    plugin.availability === 'unreleased'
      ? m['settings.plugins.unreleased_hint']({ version: plugin.version })
      : plugin.availability === 'incompatible'
        ? m['settings.plugins.incompatible_hint']({
            requires: plugin.requires ?? '',
          })
        : undefined;
  if (!reason) return undefined;
  return plugin.build
    ? `${reason} ${m['settings.plugins.installable_hint']({ version: plugin.build })}`
    : reason;
}

function matches(plugin: CataloguePlugin, query: string, repo: string) {
  if (repo !== ALL && !plugin.listedBy.some((lister) => lister.url === repo))
    return false;
  const text = query.trim().toLowerCase();
  return (
    !text ||
    [plugin.id, plugin.name, plugin.description].some((field) =>
      field.toLowerCase().includes(text),
    )
  );
}

/**
 * Everything the added repositories offer, one row per plugin and signing key.
 * A row carries the badge of every repository that lists it; installing goes
 * through the same inspect, native approval and install as any other package.
 */
export function PluginBrowse() {
  const catalogue = useQuery(pluginQueries.catalogue());
  const install = usePluginInstall();
  const [query, setQuery] = useState('');
  const [repo, setRepo] = useState(ALL);

  const repositories = catalogue.data?.repositories ?? [];
  const shown = useMemo(
    () =>
      (catalogue.data?.plugins ?? []).filter((plugin) =>
        matches(plugin, query, repo),
      ),
    [catalogue.data, query, repo],
  );

  if (catalogue.isPending) return <Bone className="h-32" />;
  if (catalogue.isError)
    return (
      <p className="text-xs text-destructive">
        {errorMessage(catalogue.error)}
      </p>
    );
  if (repositories.length === 0)
    return (
      <p className="px-3 py-2 text-xs text-muted-foreground">
        {m['settings.plugins.browse.none']()}
      </p>
    );

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <SearchInput
          className="flex-1"
          value={query}
          onChange={setQuery}
          placeholder={m['settings.plugins.browse.search']()}
        />
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

      <WarningNotice warnings={catalogue.data?.warnings} />

      {install.review && (
        <InstallReview
          inspection={install.review.inspection}
          onContinue={() => {
            if (install.review) void install.approve(install.review);
          }}
          onCancel={install.dismiss}
        />
      )}

      <div className="divide-y divide-border border border-border">
        {shown.length === 0 ? (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {m['settings.plugins.browse.empty']()}
          </p>
        ) : (
          shown.map((plugin) => (
            <CatalogueRow
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
          ))
        )}
      </div>
    </div>
  );
}

function CatalogueRow({
  plugin,
  busy,
  onInstall,
}: {
  plugin: CataloguePlugin;
  busy: boolean;
  onInstall: () => void;
}) {
  const current = plugin.installed === plugin.build;
  const action = plugin.installed
    ? m['settings.plugins.update']()
    : m['app.action.install']();

  return (
    <div className="flex items-center gap-3 px-3 py-2">
      <Thumbnail src={plugin.icon ?? undefined} icon={PuzzlePieceIcon} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-sm">
          <span className="truncate">{plugin.name || plugin.id}</span>
          {plugin.listedBy.map((lister) => (
            <Badge key={lister.url} variant="outline" title={lister.name}>
              {lister.label}
            </Badge>
          ))}
          <Badge
            variant={
              plugin.availability === 'prebuilt' ? 'outline' : 'secondary'
            }
            title={availabilityHint(plugin)}
          >
            {AVAILABILITY_LABEL[plugin.availability]()}
          </Badge>
          {plugin.installed && (
            <Badge variant="outline">
              {m['settings.plugins.installed_version']({
                version: plugin.installed,
              })}
            </Badge>
          )}
        </div>
        <div className="truncate font-mono text-[11px] text-muted-foreground">
          {[
            plugin.version,
            m['settings.plugins.key_id']({ key: plugin.keyId }),
            plugin.description,
          ]
            .filter(Boolean)
            .join(' · ')}
        </div>
      </div>
      <Button
        variant="outline"
        size="sm"
        disabled={busy || !plugin.build || current}
        onClick={onInstall}
      >
        {action}
      </Button>
    </div>
  );
}
