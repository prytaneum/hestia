import {
  PuzzlePieceIcon,
  ShieldWarningIcon,
  TrashIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import type { PluginInfo, PluginSource, PluginState } from '@/api';
import {
  Accordion,
  AccordionContent,
  AccordionItem,
} from '@/components/ui/accordion';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Switch } from '@/components/ui/switch';
import { usePluginCommands } from '@/features/plugins';
import { describeGrant } from '@/features/settings/lib';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

import { GrantList } from './plugin-grant';
import { PluginSettings } from './plugin-settings';

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
  repo: m['domain.plugin.source.repo'],
  dev: m['domain.plugin.source.dev'],
};

/**
 * One installed plugin, shaped like the other settings lists: name and badges,
 * a mono detail line, the switch and remove. Clicking the row opens what the
 * plugin is allowed to do below it.
 */
export function PluginRow({ plugin }: { plugin: PluginInfo }) {
  const enable = useMutation(pluginMutations.enable());
  const disable = useMutation(pluginMutations.disable());
  const remove = useMutation(pluginMutations.remove());
  const [open, setOpen] = useState(false);
  const commands = usePluginCommands({ plugin: plugin.manifest.id });
  const repositories = useQuery(pluginQueries.repositories());
  const store = repositories.data?.find(
    (repository) => repository.url === plugin.repository,
  );
  const { id, name, version, description, authors } = plugin.manifest;
  const label = name || id;
  const on = plugin.state === 'enabled';
  const attention =
    plugin.state === 'failed' || plugin.state === 'needs_approval';
  const reason =
    plugin.state === 'failed'
      ? plugin.lastError
      : plugin.state === 'needs_approval'
        ? m['settings.plugins.needs_approval_hint']()
        : undefined;

  return (
    <>
      <Accordion
        value={open ? [id] : []}
        onValueChange={(value) => setOpen(value.includes(id))}
      >
        <AccordionItem value={id} className="border-b-0">
          <div className="flex items-center gap-3 px-3 py-2 transition-colors hover:bg-muted/40">
            <button
              type="button"
              aria-expanded={open}
              title={m['settings.plugins.details']()}
              onClick={() => setOpen((was) => !was)}
              className="flex min-w-0 flex-1 items-center gap-3 text-left outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <PuzzlePieceIcon
                className={cn(
                  'size-4 shrink-0',
                  on ? 'text-foreground/70' : 'text-muted-foreground/50',
                )}
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 text-sm">
                  <span className="truncate">{label}</span>
                  {plugin.unrestricted.length > 0 && (
                    <Badge
                      variant="outline"
                      className="border-amber/40 text-amber"
                      title={m['settings.plugins.full_access_hint']()}
                    >
                      <ShieldWarningIcon weight="fill" />
                      {m['settings.plugins.full_access']()}
                    </Badge>
                  )}
                  {store && (
                    <Badge variant="outline" title={store.name}>
                      {store.label}
                    </Badge>
                  )}
                  {plugin.official && (
                    <Badge
                      variant="secondary"
                      title={m['settings.plugins.official_hint']()}
                    >
                      {m['settings.plugins.official']()}
                    </Badge>
                  )}
                  {plugin.obsolete && (
                    <Badge
                      variant="secondary"
                      title={m['settings.plugins.obsolete_hint']()}
                    >
                      {m['settings.plugins.obsolete']()}
                    </Badge>
                  )}
                  {plugin.source === 'dev' && (
                    <Badge
                      variant="secondary"
                      title={m['settings.plugins.dev_hint']()}
                    >
                      {m['settings.plugins.dev']()}
                    </Badge>
                  )}
                  {!on && (
                    <Badge variant={attention ? 'destructive' : 'outline'}>
                      {STATE_LABEL[plugin.state]()}
                    </Badge>
                  )}
                </div>
                <div className="truncate font-mono text-[11px] text-muted-foreground">
                  {[version, authors.join(', '), SOURCE_LABEL[plugin.source]()]
                    .filter(Boolean)
                    .join(' · ')}
                </div>
                {reason && (
                  <div className="truncate text-[11px] text-destructive">
                    {reason}
                  </div>
                )}
              </div>
            </button>

            {plugin.state !== 'needs_approval' && (
              <Switch
                size="sm"
                aria-label={
                  on ? m['app.action.disable']() : m['app.action.enable']()
                }
                checked={on}
                disabled={enable.isPending || disable.isPending}
                onCheckedChange={(next) => (next ? enable : disable).mutate(id)}
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
          <AccordionContent className="space-y-3 px-3 pt-1 pb-3 pl-10">
            {description && (
              <p className="text-xs text-muted-foreground">{description}</p>
            )}
            {commands.commands.length > 0 && (
              <div className="flex flex-wrap gap-2">
                {commands.commands.map((command) => (
                  <Button
                    key={command.command.id}
                    variant="outline"
                    size="sm"
                    title={command.command.description || undefined}
                    onClick={() => commands.run(command)}
                  >
                    {command.command.title || command.command.id}
                  </Button>
                ))}
              </div>
            )}
            <PluginSettings plugin={plugin} />
            <GrantList
              sections={describeGrant(plugin.grant, plugin.unrestricted)}
            />
          </AccordionContent>
        </AccordionItem>
      </Accordion>
      {commands.dialog}
    </>
  );
}
