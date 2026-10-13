import { useMemo, useState } from 'react';
import { toast } from 'sonner';

import { errorMessage } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Notice } from '@/components/warning-notice';
import { m } from '@/paraglide/messages.js';
import type { JobEntryKind } from '@/queries';
import { useModpack, useModpackUpdateCheck, useUpdateModpack } from '@/queries';
import { useEntryVersions } from '@/queries/versions';

export interface UpdateHandle {
  mutateAsync: (params: {
    version: string;
    allowDowngrade: boolean;
  }) => Promise<unknown>;
  isPending: boolean;
}

export function ChangeVersionDialog({
  kind,
  id,
  name,
  flavor,
  gameVersion,
  update,
  open,
  onOpenChange,
}: {
  kind: JobEntryKind;
  id: string;
  name: string;
  flavor: string;
  gameVersion: string;
  update: UpdateHandle;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const versions = useEntryVersions(kind, flavor);
  const [version, setVersion] = useState('');
  const [downgrade, setDowngrade] = useState(false);
  const [movePack, setMovePack] = useState(true);
  const [wasOpen, setWasOpen] = useState(open);

  if (wasOpen !== open) {
    setWasOpen(open);
    if (open) {
      setVersion('');
      setDowngrade(false);
      setMovePack(true);
    }
  }

  const options = useMemo(
    () => (versions.data ?? []).filter((v) => v.id !== gameVersion),
    [versions.data, gameVersion],
  );

  const pack = useModpack(kind, id).data;
  const packUpdate = useUpdateModpack(kind, id);
  const drifts = !!pack && version !== '' && version !== pack.gameVersion;
  const check = useModpackUpdateCheck(
    kind,
    id,
    drifts && pack.projectId !== '',
    version,
  );
  const fit = drifts ? check.data?.fitting : undefined;
  const pending = update.isPending || packUpdate.isPending;

  const apply = async () => {
    if (!version) return;
    try {
      if (fit && movePack)
        await packUpdate.mutateAsync({
          version: fit.versionId,
          allowDowngrade: downgrade,
        });
      else await update.mutateAsync({ version, allowDowngrade: downgrade });
      toast.success(m['app.toast.updated']({ name }));
      onOpenChange(false);
    } catch (error) {
      toast.error(errorMessage(error));
    }
  };

  return (
    <AppDialog
      open={open}
      onOpenChange={(next) => !pending && onOpenChange(next)}
      title={m['entry.settings.change_version']()}
    >
      <DialogBody className="flex flex-col gap-4">
        <Select
          value={version}
          onValueChange={(v) => setVersion(v ?? '')}
          disabled={pending}
        >
          <SelectTrigger className="w-full">
            <SelectValue placeholder={m['app.label.version']()} />
          </SelectTrigger>
          <SelectContent>
            {options.map((v) => (
              <SelectItem key={v.id} value={v.id} className="font-mono">
                {v.id}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {drifts && (
          <Notice
            message={m['entry.settings.pack_drift.notice']({
              name: pack.name,
              version: pack.versionNumber,
              game: pack.gameVersion,
              target: version,
            })}
            hint={
              !fit && !check.isFetching
                ? m['entry.settings.pack_drift.none']({ target: version })
                : undefined
            }
          />
        )}
        {fit && (
          <label
            htmlFor="move-pack"
            className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground"
          >
            <Checkbox
              id="move-pack"
              checked={movePack}
              onCheckedChange={(c) => setMovePack(c === true)}
              disabled={pending}
            />
            {m['entry.settings.pack_drift.move']({
              version: fit.versionNumber,
            })}
            {fit.channel !== 'release' && (
              <Badge variant="outline" className="text-[10px]">
                {fit.channel}
              </Badge>
            )}
          </label>
        )}
        <label
          htmlFor="allow-downgrade"
          className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground"
        >
          <Checkbox
            id="allow-downgrade"
            checked={downgrade}
            onCheckedChange={(c) => setDowngrade(c === true)}
            disabled={pending}
          />
          {m['entry.settings.allow_downgrade']()}
        </label>
      </DialogBody>
      <DialogFooter>
        <Button
          variant="outline"
          onClick={() => onOpenChange(false)}
          disabled={pending}
        >
          {m['app.action.cancel']()}
        </Button>
        <Button
          onClick={apply}
          disabled={!version || pending || (drifts && check.isFetching)}
        >
          {pending
            ? m['app.status.preparing']()
            : m['entry.settings.change_version']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}
