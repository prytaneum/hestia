import {
  ArrowsClockwiseIcon,
  DotsThreeIcon,
  FileArrowUpIcon,
  PackageIcon,
  SwapIcon,
  TrashIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import {
  dialog,
  type EntryKind,
  errorMessage,
  type ModpackFilePreview,
} from '@/api';
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
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Thumbnail } from '@/components/ui/thumbnail';
import { WarningNotice } from '@/components/warning-notice';
import { m } from '@/paraglide/messages.js';
import { modpackMutations, modpackQueries, useJobMutation } from '@/queries';
import { ProvisionProgressView } from '../components/provision-progress';
import { ModpackVersionDialog } from '../dialogs';
import { SideCard } from './cards';

/**
 * The pack an entry runs, on its overview: what it is, and the two things that
 * can be done to it. Renders nothing when the entry was not built from one.
 */
export function ModpackCard({
  kind,
  id,
  name,
  flavor,
  gameVersion,
  running,
}: {
  kind: EntryKind;
  id: string;
  name: string;
  flavor: string;
  gameVersion: string;
  running: boolean;
}) {
  const [picking, setPicking] = useState(false);
  const pack = useQuery(modpackQueries.status(kind, id));
  const update = useJobMutation(modpackMutations.update(kind, id));
  const remove = useMutation(modpackMutations.remove(kind, id));
  const queryClient = useQueryClient();
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [deleteContent, setDeleteContent] = useState(false);
  const [fromFile, setFromFile] = useState<{
    path: string;
    preview: ModpackFilePreview;
  } | null>(null);
  const check = useQuery(
    modpackQueries.updateCheck(kind, id, (pack.data?.projectId ?? '') !== ''),
  );

  if (!pack.data) return null;

  const installed = pack.data;
  // A file-sourced pack has no catalogue behind it, so only a newer file moves it.
  const updatable = installed.projectId !== '';
  const locked = update.isPending || remove.isPending || running;
  // An unreachable catalogue leaves the offer standing: the daemon is the one
  // that decides, and refusing to try would be worse than a wasted call.
  const upToDate = check.data ? !check.data.updatable : false;
  const fitting =
    check.data?.fitting?.versionId === check.data?.latestVersionId
      ? undefined
      : check.data?.fitting;

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
        <div className="flex items-center gap-2.5">
          <Thumbnail src={installed.iconUrl} icon={PackageIcon} size="lg" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium">{installed.name}</p>
            <p className="truncate text-xs text-muted-foreground">
              {installed.versionNumber || m['content.modpack.from_file']()}
            </p>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={m['app.action.more']()}
                >
                  <DotsThreeIcon weight="bold" className="size-4" />
                </Button>
              }
            />
            <DropdownMenuContent align="end" className="w-48">
              {updatable && (
                <DropdownMenuItem
                  disabled={locked}
                  onClick={() => setPicking(true)}
                >
                  <SwapIcon />
                  {m['content.change_version.action']()}
                </DropdownMenuItem>
              )}
              {updatable && (
                <DropdownMenuItem disabled={locked} onClick={updateFromFile}>
                  <FileArrowUpIcon />
                  {m['content.modpack.update_from_file.action']()}
                </DropdownMenuItem>
              )}
              {updatable && <DropdownMenuSeparator />}
              <DropdownMenuItem
                variant="destructive"
                disabled={locked}
                onClick={() => setConfirmRemove(true)}
              >
                <TrashIcon />
                {m['content.modpack.remove.action']()}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <div className="divide-y divide-border">
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

        {updatable && upToDate && !fitting && (
          <p className="text-xs text-muted-foreground">
            {m['content.modpack.up_to_date']()}
          </p>
        )}
        {fitting && (
          <p className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            {m['content.modpack.fitting']({
              version: fitting.versionNumber,
              game: fitting.gameVersion,
            })}
            {fitting.channel !== 'release' && (
              <Badge variant="outline" className="text-[10px]">
                {fitting.channel}
              </Badge>
            )}
          </p>
        )}

        {updatable && !upToDate && (
          <Button
            size="sm"
            className="w-full"
            disabled={locked || check.isLoading}
            onClick={() =>
              update
                .mutateAsync({})
                .catch((error) => toast.error(errorMessage(error)))
            }
          >
            <ArrowsClockwiseIcon data-icon="inline-start" />
            {check.data?.updatable
              ? m['content.modpack.update_to']({
                  version: check.data.latestVersionNumber,
                })
              : m['content.modpack.update']()}
          </Button>
        )}
        {!updatable && (
          <Button
            size="sm"
            variant="outline"
            className="w-full"
            disabled={locked}
            onClick={updateFromFile}
          >
            <FileArrowUpIcon data-icon="inline-start" />
            {m['content.modpack.update_from_file.action']()}
          </Button>
        )}
      </div>

      <ModpackVersionDialog
        pack={installed}
        kind={kind}
        flavor={flavor}
        gameVersion={gameVersion}
        preselect={fitting?.versionId}
        open={picking}
        onOpenChange={setPicking}
        onPick={(version, allowDowngrade) =>
          update
            .mutateAsync({ version: version.id, allowDowngrade })
            .catch((error) => toast.error(errorMessage(error)))
        }
      />

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

      <AlertDialog
        open={confirmRemove}
        onOpenChange={(open) => {
          setConfirmRemove(open);
          if (!open) setDeleteContent(false);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {m['content.modpack.remove.title']({ name: installed.name })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {m['content.modpack.remove.body']({ entry: name })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <label
            htmlFor="modpack-delete-content"
            className="flex cursor-pointer items-start gap-2 text-xs"
          >
            <Checkbox
              id="modpack-delete-content"
              className="mt-0.5"
              checked={deleteContent}
              onCheckedChange={(checked) => setDeleteContent(checked === true)}
            />
            <span>
              {m['content.modpack.remove.delete_content']()}
              <span className="block text-muted-foreground">
                {m['content.modpack.remove.delete_content_hint']()}
              </span>
            </span>
          </label>
          <AlertDialogFooter>
            <AlertDialogCancel size="sm">
              {m['app.action.cancel']()}
            </AlertDialogCancel>
            <AlertDialogAction
              size="sm"
              variant={deleteContent ? 'destructive' : 'default'}
              onClick={() => removePack(!deleteContent)}
            >
              {m['content.modpack.remove.action']()}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SideCard>
  );
}
