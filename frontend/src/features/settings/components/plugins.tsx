import { PuzzlePieceIcon, TrashIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';

import type { PluginInfo, PluginSource, PluginState } from '@/api';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Field } from '@/components/ui/field';
import { Switch } from '@/components/ui/switch';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

import { PluginInstall } from './plugin-install';

const STATE_LABEL: Record<PluginState, () => string> = {
  enabled: m['domain.plugin.state.enabled'],
  disabled: m['domain.plugin.state.disabled'],
  failed: m['domain.plugin.state.failed'],
  needs_approval: m['domain.plugin.state.needs_approval'],
};

const SOURCE_LABEL: Record<PluginSource, () => string> = {
  file: m['domain.plugin.source.file'],
  folder: m['domain.plugin.source.folder'],
  url: m['domain.plugin.source.url'],
  git: m['domain.plugin.source.git'],
  dev: m['domain.plugin.source.dev'],
};

const STATE_BADGE: Record<
  PluginState,
  'secondary' | 'outline' | 'destructive'
> = {
  enabled: 'secondary',
  disabled: 'outline',
  failed: 'destructive',
  needs_approval: 'outline',
};

/** Installed plugins, one row each, with the install control above them. */
export function PluginList() {
  const plugins = useQuery(pluginQueries.list());

  return (
    <Field>
      <PluginInstall />
      {plugins.isPending ? (
        <Bone className="h-24" />
      ) : plugins.data?.length ? (
        <div className="divide-y divide-border border border-border">
          {plugins.data.map((plugin) => (
            <PluginRow key={plugin.manifest.id} plugin={plugin} />
          ))}
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">
          {m['settings.plugins.empty']()}
        </p>
      )}
    </Field>
  );
}

function PluginRow({ plugin }: { plugin: PluginInfo }) {
  const enable = useMutation(pluginMutations.enable());
  const disable = useMutation(pluginMutations.disable());
  const remove = useMutation(pluginMutations.remove());
  const { id, name, version } = plugin.manifest;
  const label = name || id;
  const toggleable = plugin.state !== 'needs_approval';

  return (
    <div className="flex items-center gap-3 px-3 py-2">
      <PuzzlePieceIcon className="size-4 shrink-0 text-foreground/70" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-sm">
          <span className="truncate">{label}</span>
          <Badge variant={STATE_BADGE[plugin.state]}>
            {STATE_LABEL[plugin.state]()}
          </Badge>
        </div>
        <div className="font-mono text-[11px] text-muted-foreground">
          {m['settings.plugins.version']({ version })} ·{' '}
          {SOURCE_LABEL[plugin.source]()}
        </div>
        {plugin.state === 'failed' && plugin.lastError && (
          <div className="truncate text-[11px] text-destructive">
            {plugin.lastError}
          </div>
        )}
      </div>

      {toggleable && (
        <Switch
          size="sm"
          aria-label={
            plugin.state === 'enabled'
              ? m['app.action.disable']()
              : m['app.action.enable']()
          }
          checked={plugin.state === 'enabled'}
          disabled={enable.isPending || disable.isPending}
          onCheckedChange={(on) => (on ? enable : disable).mutate(id)}
        />
      )}
      <ConfirmDialog
        trigger={
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={m['app.action.remove']()}
            disabled={remove.isPending}
          >
            <TrashIcon className="size-4" />
          </Button>
        }
        title={m['settings.plugins.remove_title']({ name: label })}
        description={m['settings.plugins.remove_description']()}
        destructive
        confirmLabel={m['app.action.remove']()}
        onConfirm={() => remove.mutate(id)}
      />
    </div>
  );
}
