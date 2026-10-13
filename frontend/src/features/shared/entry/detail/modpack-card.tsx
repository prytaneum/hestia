import { PackageIcon } from '@phosphor-icons/react';
import { useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import { dialog, errorMessage, type ModpackFilePreview } from '@/api';
import { Stat } from '@/components/page';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Thumbnail } from '@/components/ui/thumbnail';
import { WarningNotice } from '@/components/warning-notice';
import { m } from '@/paraglide/messages.js';
import type { JobEntryKind } from '@/queries';
import {
  modpackQueries,
  useModpack,
  useModpackUpdateCheck,
  useRemoveModpack,
  useUpdateModpack,
} from '@/queries';
import { ProvisionProgressView } from '../components/provision-progress';
import { SideCard } from './cards';

/**
 * The pack an entry runs, on its overview: what it is, and the two things that
 * can be done to it. Renders nothing when the entry was not built from one.
 */
export function ModpackCard({
  kind,
  id,
  name,
  running,
}: {
  kind: JobEntryKind;
  id: string;
  name: string;
  running: boolean;
}) {
  const pack = useModpack(kind, id);
  const update = useUpdateModpack(kind, id);
  const remove = useRemoveModpack(kind, id);
  const queryClient = useQueryClient();
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [fromFile, setFromFile] = useState<{
    path: string;
    preview: ModpackFilePreview;
  } | null>(null);
  const check = useModpackUpdateCheck(
    kind,
    id,
    (pack.data?.projectId ?? '') !== '',
  );

  if (!pack.data) return null;

  const installed = pack.data;
  const loader = installed.loader
    ? `${installed.loader}${installed.loaderVersion ? ` ${installed.loaderVersion}` : ''}`
    : 'vanilla';
  // A file-sourced pack has no catalogue behind it, so only a newer file moves it.
  const updatable = installed.projectId !== '';
  const busy = update.isPending || remove.isPending;
  // An unreachable catalogue leaves the offer standing: the daemon is the one
  // that decides, and refusing to try would be worse than a wasted call.
  const upToDate = check.data ? !check.data.updatable : false;

  const applyFile = (path: string, preview: ModpackFilePreview) =>
    update
      .mutateAsync({
        path,
        allowDowngrade: preview.downgrade,
        allowOtherPack: !preview.samePack,
      })
      .catch((error) => toast.error(errorMessage(error)));

  const updateFromFile = async () => {
    const path = await dialog.pickPackFile();
    if (!path) return;
    try {
      const preview = await queryClient.fetchQuery(
        modpackQueries.updatePreview(kind, id, path),
      );
      if (preview.samePack && !preview.downgrade) {
        await applyFile(path, preview);
        return;
      }
      setFromFile({ path, preview });
    } catch (error) {
      toast.error(errorMessage(error));
    }
  };

  const removePack = (keepContent: boolean) =>
    remove
      .mutateAsync({ keepContent })
      .then((result) => {
        toast.success(
          keepContent
            ? m['content.modpack.released']({ files: result.releasedFiles })
            : m['content.modpack.removed']({
                files: result.removedFiles,
                overrides: result.removedOverrides,
              }),
        );
        if (result.kept.length)
          toast.info(m['content.modpack.kept']({ count: result.kept.length }));
      })
      .catch((error) => toast.error(errorMessage(error)));

  return (
    <SideCard title={m['content.modpack.title']()}>
      <div className="space-y-3">
        <div className="flex items-start gap-2.5">
          <Thumbnail src={installed.iconUrl} icon={PackageIcon} size="lg" />
          <div className="min-w-0">
            <p className="truncate text-sm font-medium">{installed.name}</p>
            <p className="truncate text-xs text-muted-foreground">
              {installed.versionNumber || m['content.modpack.from_file']()}
            </p>
          </div>
        </div>

        <div className="divide-y divide-border">
          <Stat label={m['app.label.game']()} value={installed.gameVersion} />
          <Stat label={m['app.label.loader']()} value={loader} />
          <Stat
            label={m['domain.kind.mods']()}
            value={installed.files.length}
          />
          <Stat
            label={m['content.modpack.pack_files']()}
            value={installed.overrides.length}
          />
        </div>

        {update.isPending && (
          <ProvisionProgressView
            progress={update.progress}
            fallbackLabel={m['content.modpack.update']()}
          />
        )}
        <WarningNotice warnings={update.data?.warnings} />

        {updatable && upToDate && (
          <p className="text-xs text-muted-foreground">
            {m['content.modpack.up_to_date']()}
          </p>
        )}

        <div className="flex flex-wrap gap-2">
          {updatable && !upToDate && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy || running || check.isLoading}
              onClick={() =>
                update
                  .mutateAsync({})
                  .catch((error) => toast.error(errorMessage(error)))
              }
            >
              {check.data?.updatable
                ? m['content.modpack.update_to']({
                    version: check.data.latestVersionNumber,
                  })
                : m['content.modpack.update']()}
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            disabled={busy || running}
            onClick={updateFromFile}
          >
            {m['content.modpack.update_from_file.action']()}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy || running}
            onClick={() => setConfirmRemove(true)}
          >
            {m['content.modpack.remove.action']()}
          </Button>
        </div>
      </div>

      <ConfirmDialog
        open={fromFile !== null}
        onOpenChange={(open) => !open && setFromFile(null)}
        title={m['content.modpack.update_from_file.title']({
          name: fromFile?.preview.name ?? '',
        })}
        description={
          <>
            {fromFile && !fromFile.preview.samePack && (
              <span className="block">
                {m['content.modpack.update_from_file.other_pack']({
                  given: fromFile.preview.name,
                  installed: installed.name,
                })}
              </span>
            )}
            {fromFile?.preview.downgrade && (
              <span className="block">
                {m['content.modpack.update_from_file.downgrade']({
                  from: installed.gameVersion,
                  to: fromFile.preview.gameVersion,
                })}
              </span>
            )}
          </>
        }
        destructive
        confirmLabel={m['content.modpack.update']()}
        onConfirm={() => fromFile && applyFile(fromFile.path, fromFile.preview)}
      />

      <AlertDialog open={confirmRemove} onOpenChange={setConfirmRemove}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {m['content.modpack.remove.title']({ name: installed.name })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {m['content.modpack.remove.body']({ entry: name })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel size="sm">
              {m['app.action.cancel']()}
            </AlertDialogCancel>
            <AlertDialogAction
              size="sm"
              variant="destructive"
              onClick={() => removePack(false)}
            >
              {m['content.modpack.remove.all']()}
            </AlertDialogAction>
            <AlertDialogAction size="sm" onClick={() => removePack(true)}>
              {m['content.modpack.remove.keep']()}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SideCard>
  );
}
