import {
  PuzzlePieceIcon,
  ShieldWarningIcon,
  TrashIcon,
  WarningIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';

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
import { Thumbnail } from '@/components/ui/thumbnail';
import { describeGrant } from '@/features/settings/lib';
import { bytes } from '@/lib/format';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

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
 * One installed plugin: who made it and what it does at a glance, the switch,
 * and a details drawer holding its grant, its health and the remove action. A
 * plugin with full access says so on the row, not only behind the drawer.
 */
export function PluginRow({ plugin }: { plugin: PluginInfo }) {
  const enable = useMutation(pluginMutations.enable());
  const disable = useMutation(pluginMutations.disable());
  const [open, setOpen] = useState(false);
  const { id, name, version, description } = plugin.manifest;
  const label = name || id;
  const on = plugin.state === 'enabled';
  const toggleable = plugin.state !== 'needs_approval';

  return (
    <Accordion
      value={open ? [id] : []}
      onValueChange={(value) => setOpen(value.includes(id))}
    >
      <AccordionItem value={id} className="border-b-0">
        <div className="flex items-start gap-3 px-3 py-3">
          <Thumbnail
            icon={PuzzlePieceIcon}
            size="lg"
            className={cn(!on && 'opacity-50')}
          />
          <div
            className={cn('min-w-0 flex-1 space-y-0.5', !on && 'opacity-70')}
          >
            <div className="flex min-w-0 items-baseline gap-2">
              <span className="truncate text-sm font-medium">{label}</span>
              <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                {version}
              </span>
            </div>
            {description && (
              <p className="line-clamp-2 text-xs text-muted-foreground">
                {description}
              </p>
            )}
            <StateLine plugin={plugin} />
          </div>

          <div className="flex shrink-0 items-center gap-2 self-center">
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
            {toggleable && (
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
          </div>
        </div>
        <AccordionContent className="p-0">
          {open && <PluginDetails plugin={plugin} label={label} />}
        </AccordionContent>
      </AccordionItem>
    </Accordion>
  );
}

/** Authors and source on a healthy plugin; the reason on one that needs attention. */
function StateLine({ plugin }: { plugin: PluginInfo }) {
  const { authors } = plugin.manifest;

  if (plugin.state === 'failed' || plugin.state === 'needs_approval') {
    const failed = plugin.state === 'failed';
    return (
      <p
        className={cn(
          'flex items-start gap-1.5 pt-0.5 text-xs',
          failed ? 'text-destructive' : 'text-amber',
        )}
      >
        <WarningIcon weight="fill" className="mt-px size-3.5 shrink-0" />
        <span className="line-clamp-2">
          <span className="font-medium">{STATE_LABEL[plugin.state]()}</span>
          <span className="opacity-80">
            {' — '}
            {failed
              ? plugin.lastError
              : m['settings.plugins.needs_approval_hint']()}
          </span>
        </span>
      </p>
    );
  }

  const meta = [
    authors.length > 0 &&
      m['settings.plugins.by']({ authors: authors.join(', ') }),
    SOURCE_LABEL[plugin.source](),
    plugin.state === 'disabled' && STATE_LABEL.disabled(),
  ].filter(Boolean);
  return (
    <p className="truncate pt-0.5 text-[11px] text-muted-foreground/80">
      {meta.join(' · ')}
    </p>
  );
}

function PluginDetails({
  plugin,
  label,
}: {
  plugin: PluginInfo;
  label: string;
}) {
  const remove = useMutation(pluginMutations.remove());
  const info = useQuery(pluginQueries.info(plugin.manifest.id));
  const { contributes } = plugin.manifest;
  const dataBytes = info.data?.dataBytes;

  const facts: Array<[string, string]> = [
    [
      m['settings.plugins.facts.data'](),
      dataBytes === undefined ? '…' : bytes(dataBytes),
    ],
    [m['settings.plugins.facts.calls'](), String(plugin.calls)],
    [m['settings.plugins.facts.timeouts'](), String(plugin.timeouts)],
    [m['settings.plugins.facts.traps'](), String(plugin.traps)],
  ];
  if (contributes.commands.length > 0)
    facts.push([
      m['settings.plugins.facts.commands'](),
      String(contributes.commands.length),
    ]);
  if (contributes.themes.length > 0)
    facts.push([
      m['settings.plugins.facts.themes'](),
      String(contributes.themes.length),
    ]);
  facts.push([
    m['settings.plugins.facts.source'](),
    plugin.url ?? SOURCE_LABEL[plugin.source](),
  ]);
  if (plugin.sha256)
    facts.push([
      m['settings.plugins.facts.sha256'](),
      plugin.sha256.slice(0, 16),
    ]);

  return (
    <div className="grid gap-6 border-t border-border bg-muted/20 px-3 py-4 sm:grid-cols-[1fr_13rem] sm:pl-15">
      <GrantList sections={describeGrant(plugin.grant, plugin.unrestricted)} />

      <div className="flex flex-col gap-4">
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
          {facts.map(([term, value]) => (
            <div key={term} className="contents">
              <dt className="text-muted-foreground">{term}</dt>
              <dd className="truncate text-right font-mono text-[11px] tabular-nums">
                {value}
              </dd>
            </div>
          ))}
        </dl>
        <ConfirmDialog
          trigger={
            <Button
              variant="outline"
              size="sm"
              className="self-end text-destructive hover:text-destructive"
              disabled={remove.isPending}
            >
              <TrashIcon />
              {m['app.action.remove']()}
            </Button>
          }
          title={m['settings.plugins.remove_title']({ name: label })}
          description={m['settings.plugins.remove_description']()}
          destructive
          confirmLabel={m['app.action.remove']()}
          onConfirm={() => remove.mutate(plugin.manifest.id)}
        />
      </div>
    </div>
  );
}
