import {
  ArchiveIcon,
  CaretRightIcon,
  CopyIcon,
  DotsThreeIcon,
  FileArchiveIcon,
  FileIcon,
  FilePlusIcon,
  FolderIcon,
  FolderOpenIcon,
  FolderPlusIcon,
  HouseIcon,
  PencilSimpleIcon,
  TrashIcon,
  UploadSimpleIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import { dialog, type FileEntry } from '@/api';
import { Empty } from '@/components/empty';
import { Bone } from '@/components/skeleton';
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
import { FileEditor } from '@/features/servers/components';
import { FileNameDialog, type NamePrompt } from '@/features/servers/dialogs';
import {
  archiveStem,
  freeName,
  isArchive,
  joinPath,
  pathCrumbs,
} from '@/features/servers/lib';
import { useDropzone } from '@/hooks/use-dropzone';
import { ANY_FILE } from '@/lib/file-drop';
import { agoLabel, bytes } from '@/lib/format';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { filesMutations, filesQueries } from '@/queries/files';

function Crumbs({
  path,
  onNavigate,
}: {
  path: string;
  onNavigate: (path: string) => void;
}) {
  const crumb =
    'px-1 py-0.5 font-mono text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring';
  return (
    <nav
      aria-label={m['server.files.path']()}
      className="flex min-w-0 flex-1 items-center gap-0.5"
    >
      <button
        type="button"
        aria-label={m['server.files.root']()}
        className={crumb}
        onClick={() => onNavigate('')}
      >
        <HouseIcon className="size-3.5" />
      </button>
      {pathCrumbs(path).map((part, i, all) => (
        <span key={part.path} className="flex min-w-0 items-center gap-0.5">
          <CaretRightIcon className="size-3 shrink-0 text-muted-foreground/60" />
          <button
            type="button"
            className={cn(
              crumb,
              'truncate',
              i === all.length - 1 && 'text-foreground',
            )}
            onClick={() => onNavigate(part.path)}
          >
            {part.name}
          </button>
        </span>
      ))}
    </nav>
  );
}

function entryIcon(entry: FileEntry) {
  if (entry.dir) return FolderIcon;
  return isArchive(entry.name) ? FileArchiveIcon : FileIcon;
}

/**
 * The server's game directory: browse it, edit a text file in place, and move,
 * copy, archive or remove what is there. Dropping files on the list copies
 * them into the directory on screen.
 */
export function ServerFilesTab({
  id,
  running,
  path,
  file,
  onNavigate,
  onOpenFile,
}: {
  id: string;
  running: boolean;
  /** The directory on screen; empty for the root. */
  path: string;
  /** The file open in the editor, if any. */
  file?: string;
  onNavigate: (path: string) => void;
  onOpenFile: (file?: string) => void;
}) {
  const listing = useQuery(filesQueries.list(id, path));
  const mkdir = useMutation(filesMutations.mkdir(id));
  const write = useMutation(filesMutations.write(id));
  const move = useMutation(filesMutations.move(id));
  const copy = useMutation(filesMutations.copy(id));
  const remove = useMutation(filesMutations.remove(id));
  const upload = useMutation(filesMutations.importFiles(id));
  const compress = useMutation(filesMutations.compress(id));
  const extract = useMutation(filesMutations.extract(id));

  const [prompt, setPrompt] = useState<NamePrompt | null>(null);
  const [removing, setRemoving] = useState<string[] | null>(null);
  // null = not selecting; a set of paths while the select mode is active.
  const [selected, setSelected] = useState<Set<string> | null>(null);

  const importHere = (sources: string[]) => {
    if (sources.length === 0) return;
    upload.mutate(
      { path, sources },
      {
        onSuccess: () =>
          toast.success(m['server.files.uploaded']({ count: sources.length })),
      },
    );
  };
  const { getRootProps, isDragActive } = useDropzone<HTMLDivElement>({
    accept: [ANY_FILE],
    onDrop: importHere,
    disabled: file !== undefined,
  });

  if (file !== undefined) {
    return (
      <FileEditor
        id={id}
        path={file}
        running={running}
        onClose={() => onOpenFile(undefined)}
      />
    );
  }

  const entries = listing.data ?? [];
  const names = entries.map((entry) => entry.name);
  const busy =
    move.isPending ||
    copy.isPending ||
    remove.isPending ||
    upload.isPending ||
    compress.isPending ||
    extract.isPending;

  const promptCompress = (paths: string[], suggested: string) =>
    setPrompt({
      title: m['server.files.compress.title'](),
      action: m['server.files.action.compress'](),
      initial: freeName(`${suggested}.zip`, names),
      taken: names,
      onSubmit: (name) => {
        compress.mutate({ paths, to: joinPath(path, name) });
        setSelected(null);
      },
    });

  const toggle = (entry: FileEntry) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (!next.delete(entry.path)) next.add(entry.path);
      return next;
    });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="mb-4 flex items-center gap-2">
        <Crumbs path={path} onNavigate={onNavigate} />
        {selected ? (
          <>
            <Button variant="ghost" size="sm" onClick={() => setSelected(null)}>
              {m['app.action.cancel']()}
            </Button>
            <Button
              variant="outline"
              size="sm"
              data-icon="inline-start"
              disabled={busy || selected.size === 0}
              onClick={() => promptCompress([...selected], 'archive')}
            >
              <ArchiveIcon />
              {m['server.files.action.compress']()}
            </Button>
            <Button
              variant="destructive"
              size="sm"
              data-icon="inline-start"
              disabled={busy || selected.size === 0}
              onClick={() => setRemoving([...selected])}
            >
              <TrashIcon />
              {m['server.files.delete_count']({ count: selected.size })}
            </Button>
          </>
        ) : (
          <>
            {entries.length > 0 && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setSelected(new Set())}
              >
                {m['app.action.select']()}
              </Button>
            )}
            <Button
              variant="outline"
              size="sm"
              data-icon="inline-start"
              onClick={() =>
                setPrompt({
                  title: m['server.files.new_file'](),
                  action: m['server.files.create'](),
                  initial: '',
                  taken: names,
                  onSubmit: (name) => {
                    const created = joinPath(path, name);
                    write.mutate(
                      { path: created, content: '' },
                      { onSuccess: () => onOpenFile(created) },
                    );
                  },
                })
              }
            >
              <FilePlusIcon />
              {m['server.files.new_file']()}
            </Button>
            <Button
              variant="outline"
              size="sm"
              data-icon="inline-start"
              onClick={() =>
                setPrompt({
                  title: m['server.files.new_folder'](),
                  action: m['server.files.create'](),
                  initial: '',
                  taken: names,
                  onSubmit: (name) => mkdir.mutate(joinPath(path, name)),
                })
              }
            >
              <FolderPlusIcon />
              {m['server.files.new_folder']()}
            </Button>
            <Button
              variant="outline"
              size="sm"
              data-icon="inline-start"
              disabled={upload.isPending}
              onClick={async () => importHere(await dialog.pickFiles())}
            >
              <UploadSimpleIcon />
              {m['server.files.upload']()}
            </Button>
          </>
        )}
      </div>

      <div
        {...getRootProps()}
        className={cn(
          'flex min-h-0 flex-1 flex-col',
          isDragActive && 'outline outline-1 outline-primary outline-dashed',
        )}
      >
        {listing.isPending ? (
          <Bone className="h-40" />
        ) : entries.length === 0 ? (
          <Empty
            className="flex-1"
            icon={FolderOpenIcon}
            description={m['server.files.drop_hint']()}
          >
            {m['server.files.empty']()}
          </Empty>
        ) : (
          <div className="divide-y divide-border border border-border">
            {entries.map((entry) => {
              const EntryIcon = entryIcon(entry);
              const open = () =>
                entry.dir ? onNavigate(entry.path) : onOpenFile(entry.path);
              const body = (
                <>
                  <EntryIcon
                    className={cn(
                      'size-4 shrink-0',
                      entry.dir ? 'text-ember' : 'text-muted-foreground',
                    )}
                  />
                  <span className="min-w-0 flex-1 truncate text-left text-xs">
                    {entry.name}
                  </span>
                  <span className="w-20 shrink-0 text-right font-mono text-[11px] text-muted-foreground">
                    {entry.dir ? '' : bytes(entry.size)}
                  </span>
                  <span className="w-24 shrink-0 text-right font-mono text-[11px] text-muted-foreground">
                    {agoLabel(entry.modifiedUnix)}
                  </span>
                </>
              );

              if (selected) {
                const checkId = `file-${entry.path}`;
                return (
                  <label
                    key={entry.path}
                    htmlFor={checkId}
                    className="flex cursor-pointer items-center gap-3 px-3 py-2 transition-colors hover:bg-muted/60"
                  >
                    <Checkbox
                      id={checkId}
                      checked={selected.has(entry.path)}
                      onCheckedChange={() => toggle(entry)}
                    />
                    {body}
                  </label>
                );
              }

              return (
                <div
                  key={entry.path}
                  className="flex items-center gap-1 pr-1.5 transition-colors hover:bg-muted/60"
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-3 py-2 pl-3 outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset"
                    onClick={open}
                  >
                    {body}
                  </button>
                  <DropdownMenu>
                    <DropdownMenuTrigger
                      render={
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={m['server.files.actions_for']({
                            name: entry.name,
                          })}
                        >
                          <DotsThreeIcon weight="bold" className="size-4" />
                        </Button>
                      }
                    />
                    <DropdownMenuContent align="end" className="w-44">
                      <DropdownMenuItem
                        onClick={() =>
                          setPrompt({
                            title: m['server.files.action.rename'](),
                            action: m['server.files.action.rename'](),
                            initial: entry.name,
                            taken: names,
                            onSubmit: (name) =>
                              move.mutate({
                                from: entry.path,
                                to: joinPath(path, name),
                              }),
                          })
                        }
                      >
                        <PencilSimpleIcon />
                        {m['server.files.action.rename']()}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onClick={() =>
                          setPrompt({
                            title: m['server.files.action.move'](),
                            action: m['server.files.action.move'](),
                            initial: entry.path,
                            taken: [],
                            path: true,
                            onSubmit: (to) =>
                              move.mutate({ from: entry.path, to }),
                          })
                        }
                      >
                        <FolderOpenIcon />
                        {m['server.files.action.move']()}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        disabled={busy}
                        onClick={() =>
                          copy.mutate({
                            from: entry.path,
                            to: joinPath(path, freeName(entry.name, names)),
                          })
                        }
                      >
                        <CopyIcon />
                        {m['server.files.action.duplicate']()}
                      </DropdownMenuItem>
                      {!entry.dir && isArchive(entry.name) ? (
                        <DropdownMenuItem
                          disabled={busy}
                          onClick={() =>
                            extract.mutate({
                              from: entry.path,
                              to: joinPath(
                                path,
                                freeName(archiveStem(entry.name), names),
                              ),
                            })
                          }
                        >
                          <FileArchiveIcon />
                          {m['server.files.action.extract']()}
                        </DropdownMenuItem>
                      ) : (
                        <DropdownMenuItem
                          disabled={busy}
                          onClick={() =>
                            promptCompress([entry.path], entry.name)
                          }
                        >
                          <ArchiveIcon />
                          {m['server.files.action.compress']()}
                        </DropdownMenuItem>
                      )}
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        variant="destructive"
                        onClick={() => setRemoving([entry.path])}
                      >
                        <TrashIcon />
                        {m['app.action.delete']()}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <FileNameDialog prompt={prompt} onClose={() => setPrompt(null)} />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={m['server.files.delete.title']()}
        description={m['server.files.delete.description']({
          count: removing?.length ?? 0,
        })}
        destructive
        confirmLabel={m['app.action.delete']()}
        onConfirm={() => {
          const paths = removing ?? [];
          setRemoving(null);
          setSelected(null);
          remove.mutate(paths);
        }}
      />
    </div>
  );
}
