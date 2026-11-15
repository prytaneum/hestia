import { DownloadSimpleIcon, PuzzlePieceIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import type { ReactNode } from 'react';

import type { CataloguePlugin, OutdatedPlugin, PluginInfo } from '@/api';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Switch } from '@/components/ui/switch';
import { Thumbnail } from '@/components/ui/thumbnail';
import { m } from '@/paraglide/messages.js';
import { pluginMutations } from '@/queries/plugin';

import {
  attentionReason,
  FullAccessBadge,
  SOURCE_LABEL,
  StateBadge,
  usePluginIcon,
} from './state';

export const PLUGIN_GRID = 'grid grid-cols-1 gap-3 xl:grid-cols-2';

/**
 * The card both lists share, shaped like a content card: icon, name and
 * byline, a description, then a meta line that ends in the card's one action.
 */
function Shell({
  icon,
  name,
  byline,
  badges,
  description,
  problem,
  meta,
  action,
}: {
  icon?: string;
  name: string;
  byline?: string;
  badges?: ReactNode;
  description?: string;
  problem?: string;
  meta: ReactNode;
  action: ReactNode;
}) {
  return (
    <Card
      size="sm"
      className="h-full transition-colors group-hover:bg-muted/40"
    >
      <div className="flex gap-3 px-3">
        <Thumbnail
          src={icon}
          icon={PuzzlePieceIcon}
          size="2xl"
          className="aspect-square size-auto self-stretch"
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-medium">{name}</span>
            {byline && (
              <span className="shrink-0 text-[11px] text-muted-foreground">
                {m['content.browse.by_author']({ name: byline })}
              </span>
            )}
            <span className="ml-auto flex shrink-0 items-center gap-1.5">
              {badges}
            </span>
          </div>
          <p className="mt-1 truncate text-xs text-muted-foreground">
            {description || ' '}
          </p>
          {problem && (
            <p className="line-clamp-2 text-xs text-destructive">{problem}</p>
          )}
          <div className="mt-2 flex min-h-6 items-center gap-3 text-[11px] text-muted-foreground">
            {meta}
            <span className="ml-auto flex shrink-0 items-center gap-2">
              {action}
            </span>
          </div>
        </div>
      </div>
    </Card>
  );
}

/** An installed plugin; the card opens its page. */
export function InstalledCard({
  plugin,
  update,
  busy,
  onUpdate,
}: {
  plugin: PluginInfo;
  update?: OutdatedPlugin;
  busy: boolean;
  onUpdate: (offer: OutdatedPlugin) => void;
}) {
  const enable = useMutation(pluginMutations.enable());
  const disable = useMutation(pluginMutations.disable());
  const icon = usePluginIcon(plugin);
  const { id, name, version, description, authors } = plugin.manifest;
  const on = plugin.state === 'enabled';

  return (
    <Link
      to="/plugins/$plugin"
      params={{ plugin: id }}
      className="group block outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <Shell
        icon={icon}
        name={name || id}
        byline={authors.join(', ') || undefined}
        badges={
          <>
            <FullAccessBadge plugin={plugin} />
            <StateBadge plugin={plugin} />
          </>
        }
        description={description}
        problem={attentionReason(plugin)}
        meta={
          <>
            <span className="font-mono">{version}</span>
            <span className="truncate">{SOURCE_LABEL[plugin.source]()}</span>
          </>
        }
        action={
          // biome-ignore lint/a11y/noStaticElementInteractions: keeps the controls from following the card's link
          // biome-ignore lint/a11y/useKeyWithClickEvents: the controls inside handle their own keys
          <span
            className="flex items-center gap-2"
            onClick={(event) => event.preventDefault()}
          >
            {update && (
              <Button
                size="xs"
                variant="outline"
                disabled={busy || !update.apiCompatible}
                title={
                  update.apiCompatible
                    ? undefined
                    : m['settings.plugins.update_needs_api']({
                        version: update.available,
                        api: update.requires ?? '',
                      })
                }
                onClick={() => onUpdate(update)}
              >
                {m['settings.plugins.update_to']({ version: update.available })}
              </Button>
            )}
            {plugin.state !== 'needs_approval' &&
              plugin.state !== 'needs_update' && (
                <Switch
                  size="sm"
                  aria-label={
                    on ? m['app.action.disable']() : m['app.action.enable']()
                  }
                  checked={on}
                  disabled={enable.isPending || disable.isPending}
                  onCheckedChange={(next) =>
                    (next ? enable : disable).mutate(id)
                  }
                />
              )}
          </span>
        }
      />
    </Link>
  );
}

const AVAILABILITY_HINT: Record<
  CataloguePlugin['availability'],
  (plugin: CataloguePlugin) => string | undefined
> = {
  prebuilt: () => undefined,
  unreleased: (plugin) =>
    m['settings.plugins.unreleased_hint']({ version: plugin.version }),
  incompatible: (plugin) =>
    m['settings.plugins.incompatible_hint']({
      requires: plugin.requires ?? '',
    }),
};

const AVAILABILITY_LABEL: Record<
  CataloguePlugin['availability'],
  () => string
> = {
  prebuilt: m['domain.plugin.availability.prebuilt'],
  unreleased: m['domain.plugin.availability.unreleased'],
  incompatible: m['domain.plugin.availability.incompatible'],
};

function availabilityHint(plugin: CataloguePlugin): string | undefined {
  const reason = AVAILABILITY_HINT[plugin.availability](plugin);
  if (!reason) return undefined;
  return plugin.build
    ? `${reason} ${m['settings.plugins.installable_hint']({ version: plugin.build })}`
    : reason;
}

/** A plugin a repository offers; one that is installed opens its page. */
export function CatalogueCard({
  plugin,
  busy,
  onInstall,
}: {
  plugin: CataloguePlugin;
  busy: boolean;
  onInstall: () => void;
}) {
  const current = plugin.installed === plugin.build;
  const card = (
    <Shell
      icon={plugin.icon ?? undefined}
      name={plugin.name || plugin.id}
      byline={plugin.listedBy.map((lister) => lister.name).join(', ')}
      badges={
        <>
          {plugin.availability !== 'prebuilt' && (
            <Badge variant="secondary" title={availabilityHint(plugin)}>
              {AVAILABILITY_LABEL[plugin.availability]()}
            </Badge>
          )}
          {plugin.installed && (
            <Badge variant="outline">
              {m['settings.plugins.installed_version']({
                version: plugin.installed,
              })}
            </Badge>
          )}
        </>
      }
      description={plugin.description}
      meta={
        <>
          <span className="font-mono">{plugin.version}</span>
          <span className="truncate font-mono">
            {m['settings.plugins.key_id']({ key: plugin.keyId })}
          </span>
        </>
      }
      action={
        !current && (
          <Button
            size="xs"
            variant="outline"
            data-icon="inline-start"
            disabled={busy || !plugin.build}
            onClick={(event) => {
              event.preventDefault();
              onInstall();
            }}
          >
            <DownloadSimpleIcon weight="bold" />
            {plugin.installed
              ? m['settings.plugins.update']()
              : m['app.action.install']()}
          </Button>
        )
      }
    />
  );

  if (!plugin.installed) return <div className="group">{card}</div>;
  return (
    <Link
      to="/plugins/$plugin"
      params={{ plugin: plugin.id }}
      className="group block outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      {card}
    </Link>
  );
}
