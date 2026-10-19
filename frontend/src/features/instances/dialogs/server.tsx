import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';

import type { ServerEntry } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Spinner } from '@/components/ui/spinner';
import { Switch } from '@/components/ui/switch';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { instanceMutations } from '@/queries/instance';

/**
 * Add or edit one entry of the multiplayer list. `server` is `null` to add, an
 * entry to edit, and `undefined` when the dialog is closed.
 */
export function ServerEntryDialog({
  instance,
  server,
  onOpenChange,
}: {
  instance: string;
  server: ServerEntry | null | undefined;
  onOpenChange: (open: boolean) => void;
}) {
  const edit = useMutation(instanceMutations.serverEdit(instance));
  const [name, setName] = useState('');
  const [address, setAddress] = useState('');
  const [acceptTextures, setAcceptTextures] = useState(false);
  const open = server !== undefined;

  // Re-seed the fields from whichever entry the dialog was opened on.
  const [seeded, setSeeded] = useState<ServerEntry | null | undefined>(
    undefined,
  );
  if (open && seeded !== server) {
    setSeeded(server);
    setName(server?.name ?? '');
    setAddress(server?.address ?? '');
    setAcceptTextures(server?.acceptTextures ?? false);
  }

  const submit = () => {
    edit.mutate(
      {
        server: server ? server.name || server.address : undefined,
        name,
        address,
        acceptTextures,
      },
      {
        onSuccess: (written) => {
          notify.warnings(written.warnings, { id: 'servers-list:write' });
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      title={
        server
          ? m['instance.servers.edit.title']({ name: server.name })
          : m['instance.servers.add.title']()
      }
    >
      <DialogBody className="space-y-4">
        <div className="space-y-2">
          <Label htmlFor="server-name">{m['app.label.name']()}</Label>
          <Input
            id="server-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="server-address">{m['app.label.address']()}</Label>
          <Input
            id="server-address"
            value={address}
            placeholder="mc.example.net"
            onChange={(e) => setAddress(e.target.value)}
          />
        </div>
        <div className="flex items-center justify-between gap-3">
          <Label htmlFor="server-textures">
            {m['instance.servers.textures']()}
          </Label>
          <Switch
            id="server-textures"
            checked={acceptTextures}
            onCheckedChange={setAcceptTextures}
          />
        </div>
      </DialogBody>
      <DialogFooter>
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          {m['app.action.cancel']()}
        </Button>
        <Button
          disabled={!name.trim() || !address.trim() || edit.isPending}
          onClick={submit}
        >
          {edit.isPending ? <Spinner /> : m['app.action.apply']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}
