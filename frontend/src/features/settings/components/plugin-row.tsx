import { PuzzlePieceIcon, TrashIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';

import type { PluginInfo, PluginSource, PluginState } from '@/api';
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Switch } from '@/components/ui/switch';
import { describeGrant } from '@/features/settings/lib';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { pluginMutations } from '@/queries/plugin';

import { GrantList } from './plugin-grant';

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

/**
 * One installed plugin, shaped like the other settings lists: name and badges,
 * a mono detail line, the switch and remove. What it is allowed to do opens
 * below the row.
 */
export function PluginRow({ plugin }: { plugin: PluginInfo }) {
  const enable = useMutation(pluginMutations.enable());
  const disable = useMutation(pluginMutations.disable());
  const remove = useMutation(pluginMutations.remove());
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
    <Accordion>
      <AccordionItem className="border-b-0">
        <div className="flex items-center gap-3 px-3 py-2">
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
                  title={m['settings.plugins.full_access_hint']()}
                >
                  {m['settings.plugins.full_access']()}
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
          <AccordionTrigger
            aria-label={m['settings.plugins.details']()}
            className="flex-none py-0 text-muted-foreground hover:no-underline"
          />
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
        <AccordionContent className="space-y-3 px-3 pb-3 pl-10">
          {description && (
            <p className="text-xs text-muted-foreground">{description}</p>
          )}
          <GrantList
            sections={describeGrant(plugin.grant, plugin.unrestricted)}
          />
        </AccordionContent>
      </AccordionItem>
    </Accordion>
  );
}
