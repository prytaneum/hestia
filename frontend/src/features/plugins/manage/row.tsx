import { CaretRightIcon, PuzzlePieceIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';

import type { OutdatedPlugin, PluginInfo } from '@/api';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Thumbnail } from '@/components/ui/thumbnail';
import { m } from '@/paraglide/messages.js';
import { pluginMutations } from '@/queries/plugin';

import {
  attentionReason,
  FullAccessBadge,
  StateBadge,
  usePluginIcon,
} from './state';

/**
 * One installed plugin: what it is, whether it runs, and the one action it may
 * be waiting on. Everything else about it is on its own page, which the row
 * opens.
 */
export function PluginRow({
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
  const reason = attentionReason(plugin);

  return (
    <div className="flex items-center gap-3 px-3 py-2.5 transition-colors hover:bg-muted/40">
      <Link
        to="/plugins/$plugin"
        params={{ plugin: id }}
        className="flex min-w-0 flex-1 items-center gap-3 outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <Thumbnail src={icon} icon={PuzzlePieceIcon} size="md" />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-sm">
            <span className="truncate">{name || id}</span>
            <StateBadge plugin={plugin} />
            <FullAccessBadge plugin={plugin} />
          </div>
          <div className="truncate text-xs text-muted-foreground">
            {description || authors.join(', ') || id}
          </div>
          {reason && (
            <div className="line-clamp-2 text-xs text-destructive">
              {reason}
            </div>
          )}
        </div>
        <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
          {version}
        </span>
      </Link>

      {update && (
        <Button
          variant="outline"
          size="sm"
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
      {plugin.state !== 'needs_approval' && plugin.state !== 'needs_update' && (
        <Switch
          size="sm"
          aria-label={on ? m['app.action.disable']() : m['app.action.enable']()}
          checked={on}
          disabled={enable.isPending || disable.isPending}
          onCheckedChange={(next) => (next ? enable : disable).mutate(id)}
        />
      )}
      <CaretRightIcon className="size-3.5 shrink-0 text-muted-foreground/60" />
    </div>
  );
}
