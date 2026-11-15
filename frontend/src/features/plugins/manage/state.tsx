import { ShieldWarningIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';

import type { PluginInfo, PluginSource, PluginState } from '@/api';
import { Badge } from '@/components/ui/badge';
import { m } from '@/paraglide/messages.js';
import { pluginQueries } from '@/queries/plugin';

const STATE_LABEL: Record<PluginState, () => string> = {
  enabled: m['domain.plugin.state.enabled'],
  disabled: m['domain.plugin.state.disabled'],
  failed: m['domain.plugin.state.failed'],
  needs_approval: m['domain.plugin.state.needs_approval'],
  needs_update: m['domain.plugin.state.needs_update'],
};

export const SOURCE_LABEL: Record<PluginSource, () => string> = {
  file: m['domain.plugin.source.file'],
  folder: m['domain.plugin.source.folder'],
  url: m['domain.plugin.source.url'],
  repo: m['domain.plugin.source.repo'],
  dev: m['domain.plugin.source.dev'],
};

/** Whether every word of `query` appears somewhere in `fields`. */
export function matchesQuery(query: string, fields: string[]): boolean {
  const haystack = fields.join(' ').toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((term) => haystack.includes(term));
}

/** A state the user has to act on before the plugin runs again. */
export function needsAttention({ state }: PluginInfo): boolean {
  return (
    state === 'failed' || state === 'needs_approval' || state === 'needs_update'
  );
}

/** Why a plugin needs attention, in the daemon's words where it gave any. */
export function attentionReason(plugin: PluginInfo): string | undefined {
  if (plugin.state === 'needs_approval')
    return m['settings.plugins.needs_approval_hint']();
  return needsAttention(plugin) ? plugin.lastError : undefined;
}

/** The plugin's state when it is anything but running. */
export function StateBadge({ plugin }: { plugin: PluginInfo }) {
  if (plugin.state === 'enabled') return null;
  return (
    <Badge variant={needsAttention(plugin) ? 'destructive' : 'outline'}>
      {STATE_LABEL[plugin.state]()}
    </Badge>
  );
}

export function FullAccessBadge({ plugin }: { plugin: PluginInfo }) {
  if (plugin.unrestricted.length === 0) return null;
  return (
    <Badge
      variant="outline"
      className="border-amber/40 text-amber"
      title={m['settings.plugins.full_access_hint']()}
    >
      <ShieldWarningIcon weight="fill" />
      {m['settings.plugins.full_access']()}
    </Badge>
  );
}

/** The icon its repository lists for an installed plugin, matched by signing key. */
export function usePluginIcon(plugin: PluginInfo): string | undefined {
  const { data } = useQuery(pluginQueries.catalogue());
  return (
    data?.plugins.find(
      (listed) =>
        listed.id === plugin.manifest.id && listed.keyId === plugin.signer,
    )?.icon ?? undefined
  );
}
