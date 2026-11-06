import { FileArrowUpIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';

import { dialog } from '@/api';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription } from '@/components/ui/field';
import { Tabs, TabsChip, TabsContent, TabsList } from '@/components/ui/tabs';
import { useDropzone } from '@/hooks/use-dropzone';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useDaemon } from '@/queries/daemon';
import { pluginQueries } from '@/queries/plugin';
import { PluginBrowse } from './plugin-browse';
import {
  InstallFromLink,
  InstallReview,
  usePluginInstall,
} from './plugin-install';
import { PluginRow } from './plugin-row';

/** Installed and Browse, one level below the Plugins tab. */
export function PluginsPanel() {
  return (
    <Tabs defaultValue="installed" className="gap-3">
      <TabsList variant="line" className="h-auto justify-start gap-1.5">
        <TabsChip value="installed">
          {m['settings.plugins.tabs.installed']()}
        </TabsChip>
        <TabsChip value="browse">
          {m['settings.plugins.tabs.browse']()}
        </TabsChip>
      </TabsList>
      <TabsContent value="installed">
        <PluginList />
      </TabsContent>
      <TabsContent value="browse">
        <PluginBrowse />
      </TabsContent>
    </Tabs>
  );
}

/** Installed plugins, one row each; the list also takes a dropped `.hplugin`. */
export function PluginList() {
  const plugins = useQuery(pluginQueries.list());
  const daemon = useDaemon();
  const install = usePluginInstall();
  const { getRootProps, isDragActive } = useDropzone<HTMLDivElement>({
    accept: dialog.PLUGIN_EXTENSIONS,
    multiple: false,
    label: m['settings.plugins.drop'](),
    onDrop: ([path]) => void install.start({ path }),
  });

  const pickFile = async () => {
    const [path] = await dialog.pickPluginPackage();
    if (path) await install.start({ path });
  };

  if (plugins.isPending) return <Bone className="h-32" />;
  const installed = plugins.data ?? [];

  return (
    <Field>
      <div className="flex items-center justify-between gap-3">
        <FieldDescription>
          {daemon.status?.safeMode
            ? m['app.daemon.safe_mode_hint']()
            : m['settings.plugins.hint']()}
        </FieldDescription>
        <div className="flex shrink-0 items-center gap-2">
          <InstallFromLink
            busy={install.busy}
            onSubmit={(url) => void install.start({ url })}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={install.busy}
            onClick={() => void pickFile()}
          >
            <FileArrowUpIcon />
            {m['settings.plugins.install_file']()}
          </Button>
        </div>
      </div>

      {install.review && (
        <InstallReview
          inspection={install.review.inspection}
          onContinue={() => {
            if (install.review) void install.approve(install.review);
          }}
          onCancel={install.dismiss}
        />
      )}

      <div
        {...getRootProps()}
        className={cn(
          'divide-y divide-border border border-border',
          isDragActive && 'border-dashed border-primary',
        )}
      >
        {installed.length === 0 ? (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {m['settings.plugins.empty']()}
          </p>
        ) : (
          installed.map((plugin) => (
            <PluginRow key={plugin.manifest.id} plugin={plugin} />
          ))
        )}
      </div>
    </Field>
  );
}
