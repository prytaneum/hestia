import { PuzzlePieceIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import { dialog, type OutdatedPlugin, type PluginInfo } from '@/api';
import { Empty } from '@/components/empty';
import { Section } from '@/components/page';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { useConfig } from '@/features/settings/use-config';
import { useDropzone } from '@/hooks/use-dropzone';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { pluginQueries } from '@/queries/plugin';

import { PluginDeveloper } from './developer';
import type { PluginInstall } from './install';
import { PluginRow } from './row';
import { needsAttention } from './state';

interface Listed {
  plugin: PluginInfo;
  update?: OutdatedPlugin;
}

/**
 * Installed plugins in the order they want the user: the ones that stopped,
 * the ones with an update, then the rest. The tab also takes a dropped
 * `.hplugin`.
 */
export function Installed({
  install,
  onBrowse,
}: {
  install: PluginInstall;
  onBrowse: () => void;
}) {
  const plugins = useQuery(pluginQueries.list());
  const outdated = useQuery(pluginQueries.outdated());
  const { entries } = useConfig();
  const [updatingAll, setUpdatingAll] = useState(false);
  const { getRootProps, isDragActive } = useDropzone<HTMLDivElement>({
    accept: dialog.PLUGIN_EXTENSIONS,
    multiple: false,
    label: m['settings.plugins.drop'](),
    onDrop: ([path]) => void install.start({ path }),
  });

  if (plugins.isPending) return <Bone className="h-32" />;

  const listed: Listed[] = (plugins.data ?? []).map((plugin) => ({
    plugin,
    update: outdated.data?.plugins.find(
      (offer) => offer.id === plugin.manifest.id,
    ),
  }));
  const attention = listed.filter(({ plugin }) => needsAttention(plugin));
  const updates = listed.filter(
    ({ plugin, update }) => update && !needsAttention(plugin),
  );
  const rest = listed.filter(
    ({ plugin, update }) => !update && !needsAttention(plugin),
  );
  const busy = install.busy || updatingAll;

  const updateAll = async () => {
    setUpdatingAll(true);
    try {
      for (const { plugin, update } of updates) {
        if (!update?.apiCompatible) continue;
        if (!(await install.update(update, plugin.url))) break;
      }
    } finally {
      setUpdatingAll(false);
    }
  };

  const group = (title: string, rows: Listed[], action?: React.ReactNode) =>
    rows.length > 0 && (
      <Section title={title} count={rows.length} action={action}>
        <div className="divide-y divide-border border border-border">
          {rows.map(({ plugin, update }) => (
            <PluginRow
              key={plugin.manifest.id}
              plugin={plugin}
              update={update}
              busy={busy}
              onUpdate={(offer) => void install.update(offer, plugin.url)}
            />
          ))}
        </div>
      </Section>
    );

  return (
    <div
      {...getRootProps()}
      className={cn(
        'space-y-6 border border-transparent',
        isDragActive && 'border-dashed border-primary',
      )}
    >
      {listed.length === 0 && (
        <Empty
          icon={PuzzlePieceIcon}
          description={m['plugins.manage.empty_hint']()}
          action={
            <Button variant="outline" size="sm" onClick={onBrowse}>
              {m['plugins.manage.browse']()}
            </Button>
          }
        >
          {m['settings.plugins.empty']()}
        </Empty>
      )}
      {group(m['plugins.manage.group.attention'](), attention)}
      {group(
        m['plugins.manage.group.updates'](),
        updates,
        updates.some(({ update }) => update?.apiCompatible) && (
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => void updateAll()}
          >
            {m['plugins.manage.update_all']()}
          </Button>
        ),
      )}
      {group(m['settings.plugins.tabs.installed'](), rest)}
      {entries.developer && (
        <Section title={m['settings.plugins.developer.title']()}>
          <PluginDeveloper />
        </Section>
      )}
    </div>
  );
}
