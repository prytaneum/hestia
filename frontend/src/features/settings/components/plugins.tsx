import { FileArrowUpIcon, PuzzlePieceIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';

import { dialog } from '@/api';
import { Empty } from '@/components/empty';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { useDropzone } from '@/hooks/use-dropzone';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { pluginQueries } from '@/queries/plugin';

import {
  InstallFromLink,
  InstallReview,
  usePluginInstall,
} from './plugin-install';
import { PluginRow } from './plugin-row';

/**
 * Installed plugins under one toolbar: a count and the two ways in, a file or a
 * link. The whole list takes a dropped `.hplugin`, so the install surface costs
 * no space once there is something to manage.
 */
export function PluginList() {
  const plugins = useQuery(pluginQueries.list());
  const install = usePluginInstall();
  const { getRootProps, isDragActive } = useDropzone<HTMLDivElement>({
    accept: dialog.PLUGIN_EXTENSIONS,
    multiple: false,
    label: m['settings.plugins.drop'](),
    onDrop: ([path]) => void install.start({ path }),
  });
  const installed = plugins.data ?? [];

  const pickFile = async () => {
    const [path] = await dialog.pickPluginPackage();
    if (path) await install.start({ path });
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs text-muted-foreground">
          {plugins.isPending
            ? ' '
            : m['settings.plugins.count']({ count: installed.length })}
        </span>
        <div className="flex items-center gap-2">
          <InstallFromLink
            busy={install.busy}
            onSubmit={(url) => void install.start({ url })}
          />
          <Button
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
          'border border-border transition-colors',
          isDragActive && 'border-dashed border-primary bg-primary/5',
        )}
      >
        {plugins.isPending ? (
          <div className="space-y-px p-3">
            <Bone className="h-12" />
            <Bone className="h-12" />
          </div>
        ) : installed.length > 0 ? (
          <div className="divide-y divide-border">
            {installed.map((plugin) => (
              <PluginRow key={plugin.manifest.id} plugin={plugin} />
            ))}
          </div>
        ) : (
          <Empty
            icon={PuzzlePieceIcon}
            description={m['settings.plugins.empty_hint']()}
          >
            {m['settings.plugins.empty']()}
          </Empty>
        )}
      </div>
    </div>
  );
}
