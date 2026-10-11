import { FileArrowUpIcon, WarningIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import type { ImportFormat } from '@/api';
import { dialog, errorMessage, errorMessageFromInfo } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Field, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Spinner } from '@/components/ui/spinner';
import { useDropTarget } from '@/hooks/use-drop-target';
import { cn } from '@/lib/utils';
import { toastWarnings } from '@/lib/warnings';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { transferMutations, transferQueries } from '@/queries/transfer';

/**
 * Import an instance from an archive. The format is the daemon's to work out —
 * the dialog asks for a file and reports what it turned out to be, because
 * somebody handed an archive should not have to know which launcher made it.
 *
 * `initialPath` prefills the file, for the paths that already have one: a drop
 * onto the library, or a file the shell opened with.
 */
export function ImportInstanceDialog({
  open,
  onOpenChange,
  initialPath = '',
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  initialPath?: string;
}) {
  const [path, setPath] = useState(initialPath);
  const [name, setName] = useState('');
  const [renamed, setRenamed] = useState(false);

  useEffect(() => {
    if (open) setPath(initialPath);
  }, [open, initialPath]);

  const archive = useQuery(transferQueries.archive(path));
  const run = useJobMutation(transferMutations.import());

  // The archive's own name is the default, until the user types over it. A
  // taken name is pre-resolved rather than reported after the fact.
  useEffect(() => {
    if (renamed || !archive.data) return;
    setName(
      archive.data.nameTaken
        ? `${archive.data.name} (imported)`
        : archive.data.name,
    );
  }, [archive.data, renamed]);

  const select = (archivePath: string) => {
    setPath(archivePath);
    setRenamed(false);
  };

  const choose = async () => {
    const picked = await dialog.pickInstanceArchive();
    if (picked) select(picked);
  };

  const dropping = useDropTarget({
    extensions: dialog.ARCHIVE_EXTENSIONS,
    enabled: open,
    onDrop: ([archive]) => select(archive),
  });

  const start = () => {
    onOpenChange(false);
    run.mutate(
      { path, name },
      {
        onSuccess: (done) => {
          toast.success(
            m['instance.import.done']({ name: done.instance.name }),
          );
          for (const failure of done.failures) {
            toast.error(failure.title, {
              description: errorMessageFromInfo(failure.error),
            });
          }
          toastWarnings(done.warnings);
        },
        onError: (error) => toast.error(errorMessage(error)),
      },
    );
  };

  const unreadable = path.length > 0 && archive.isError;

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      title={m['instance.import.title']()}
      description={m['instance.import.description']()}
    >
      <DialogBody className="flex flex-col gap-4">
        <Button
          variant="outline"
          className={cn(
            'h-auto justify-start gap-3 py-3 text-left',
            dropping && 'border-primary border-dashed bg-primary/5',
          )}
          onClick={choose}
        >
          <FileArrowUpIcon className="size-5 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1">
            <span className="block truncate font-medium">
              {path ? fileName(path) : m['instance.import.choose']()}
            </span>
            <span className="block truncate text-muted-foreground text-xs">
              {path || m['instance.import.formats']()}
            </span>
          </span>
        </Button>

        {archive.isFetching && (
          <p className="flex items-center gap-2 text-muted-foreground text-sm">
            <Spinner className="size-4" />
            {m['instance.import.reading']()}
          </p>
        )}

        {unreadable && (
          <p className="flex items-start gap-2 text-destructive text-sm">
            <WarningIcon className="mt-0.5 size-4 shrink-0" />
            {errorMessage(archive.error)}
          </p>
        )}

        {archive.data && !archive.isFetching && (
          <>
            <div className="flex flex-wrap items-center gap-2 rounded-md border bg-muted/30 p-3 text-sm">
              <Badge variant="secondary">
                {m[
                  `domain.import_format.${archive.data.format as ImportFormat}`
                ]()}
              </Badge>
              <span className="text-muted-foreground">
                {archive.data.loader
                  ? `${archive.data.loader} ${archive.data.loaderVersion}`
                  : 'vanilla'}
              </span>
              <span className="text-muted-foreground">
                {archive.data.gameVersion}
              </span>
            </div>

            <Field>
              <FieldLabel htmlFor="import-name">
                {m['app.label.name']()}
              </FieldLabel>
              <Input
                id="import-name"
                value={name}
                onChange={(event) => {
                  setRenamed(true);
                  setName(event.target.value);
                }}
              />
            </Field>
          </>
        )}
      </DialogBody>

      <DialogFooter>
        <Button variant="outline" onClick={() => onOpenChange(false)}>
          {m['app.action.cancel']()}
        </Button>
        <Button
          onClick={start}
          disabled={!archive.data || archive.isFetching || !name.trim()}
        >
          {m['app.action.import']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}

function fileName(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}
