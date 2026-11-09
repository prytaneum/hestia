import { ArrowClockwiseIcon, FolderOpenIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';

import { dialog, errorMessage, onTopic, type PluginInfo } from '@/api';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription } from '@/components/ui/field';
import { appendDevLog, type DevLogLine } from '@/features/settings/lib';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

import { usePluginInstall } from './plugin-install';

const STATS_REFRESH_MS = 3000;

/** The plugin's `plugin.log` lines since this view mounted. */
function useDevLog(id: string): DevLogLine[] {
  const [lines, setLines] = useState<DevLogLine[]>([]);
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    onTopic('plugin.log', (event) => {
      if (event.id === id)
        setLines((held) =>
          appendDevLog(held, { level: event.level, text: event.text }),
        );
    })
      .then((unsubscribe) => {
        if (cancelled) unsubscribe();
        else off = unsubscribe;
      })
      .catch(() => {
        // Outside the Tauri shell there are no daemon events to hear.
      });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [id]);
  return lines;
}

/** Developer mode's half of the Plugins tab: folder plugins, their log and their counters. */
export function PluginDeveloper() {
  const plugins = useQuery({
    ...pluginQueries.list(),
    refetchInterval: STATS_REFRESH_MS,
  });
  const install = usePluginInstall();
  const folderPlugins = (plugins.data ?? []).filter(
    (plugin) => plugin.source === 'dev',
  );

  const loadFolder = async () => {
    const path = await dialog.pickFolder();
    if (path) await install.start({ path }, true);
  };

  return (
    <Field>
      <div className="flex items-center justify-between gap-3">
        <FieldDescription>
          {m['settings.plugins.developer.hint']()}
        </FieldDescription>
        <Button
          variant="outline"
          size="sm"
          disabled={install.busy}
          onClick={() => void loadFolder()}
        >
          <FolderOpenIcon />
          {m['settings.plugins.developer.load']()}
        </Button>
      </div>
      <div className="divide-y divide-border border border-border">
        {folderPlugins.length === 0 ? (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {m['settings.plugins.developer.empty']()}
          </p>
        ) : (
          folderPlugins.map((plugin) => (
            <DevPluginRow key={plugin.manifest.id} plugin={plugin} />
          ))
        )}
      </div>
    </Field>
  );
}

function DevPluginRow({ plugin }: { plugin: PluginInfo }) {
  const { id, name } = plugin.manifest;
  const enable = useMutation(pluginMutations.enable());
  const disable = useMutation(pluginMutations.disable());
  const lines = useDevLog(id);
  const reload = async () => {
    try {
      await disable.mutateAsync(id);
      await enable.mutateAsync(id);
    } catch (error) {
      notify.error(errorMessage(error));
    }
  };
  const { developer } = plugin;

  return (
    <div className="space-y-2 px-3 py-2">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm">
            <span className="truncate">{name || id}</span>
            <Badge variant="outline">{plugin.state}</Badge>
          </div>
          <div className="font-mono text-[11px] text-muted-foreground">
            {m['settings.plugins.developer.stats']({
              calls: plugin.calls,
              traps: plugin.traps,
              timeouts: plugin.timeouts,
            })}
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={enable.isPending || disable.isPending}
          onClick={() => void reload()}
        >
          <ArrowClockwiseIcon />
          {m['settings.plugins.developer.reload']()}
        </Button>
      </div>
      {developer && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 font-mono text-[11px] text-muted-foreground">
          <dt>{m['settings.plugins.developer.digest']()}</dt>
          <dd className="truncate">{developer.grantDigest}</dd>
          <dt>{m['settings.plugins.developer.cache']()}</dt>
          <dd className="truncate">{developer.cacheDir}</dd>
          {developer.errorChain.length > 0 && (
            <>
              <dt>{m['settings.plugins.developer.error_chain']()}</dt>
              <dd className="text-destructive">
                {developer.errorChain.join(' → ')}
              </dd>
            </>
          )}
        </dl>
      )}
      <div
        role="log"
        aria-label={m['settings.plugins.developer.logs']()}
        className="max-h-40 overflow-auto bg-muted/40 p-2 font-mono text-[11px]"
      >
        {lines.length === 0 ? (
          <span className="text-muted-foreground">
            {m['settings.plugins.developer.log_empty']()}
          </span>
        ) : (
          lines.map((line, index) => (
            <div
              // biome-ignore lint/suspicious/noArrayIndexKey: an append-only tail has no other identity
              key={index}
              className={
                line.level === 'error' ? 'text-destructive' : undefined
              }
            >
              {line.text}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
