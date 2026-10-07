import type { UseQueryOptions } from '@tanstack/react-query';
import { useQuery } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { toast } from 'sonner';

import type { GameVersion } from '@/api';
import { errorMessage } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { m } from '@/paraglide/messages.js';

export interface UpdateHandle {
  mutateAsync: (params: {
    version: string;
    allowDowngrade: boolean;
  }) => Promise<unknown>;
  isPending: boolean;
}

export function ChangeVersionDialog({
  name,
  gameVersion,
  versionsQuery,
  update,
  open,
  onOpenChange,
}: {
  name: string;
  gameVersion: string;
  // biome-ignore lint/suspicious/noExplicitAny: the query factories' option types differ per domain.
  versionsQuery: UseQueryOptions<GameVersion[], any, GameVersion[], any>;
  update: UpdateHandle;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const versions = useQuery(versionsQuery);
  const [version, setVersion] = useState('');
  const [downgrade, setDowngrade] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);

  if (wasOpen !== open) {
    setWasOpen(open);
    if (open) {
      setVersion('');
      setDowngrade(false);
    }
  }

  const options = useMemo(
    () => (versions.data ?? []).filter((v) => v.id !== gameVersion),
    [versions.data, gameVersion],
  );

  const pending = update.isPending;

  const apply = async () => {
    if (!version) return;
    try {
      await update.mutateAsync({ version, allowDowngrade: downgrade });
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
        <Button onClick={apply} disabled={!version || pending}>
          {pending
            ? m['app.status.preparing']()
            : m['entry.settings.change_version']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}
