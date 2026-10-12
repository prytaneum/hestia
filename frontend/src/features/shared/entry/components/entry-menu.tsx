import {
  ArrowClockwiseIcon,
  FolderOpenIcon,
  GearSixIcon,
  PushPinIcon,
  PushPinSlashIcon,
  TrashIcon,
  UploadSimpleIcon,
} from '@phosphor-icons/react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { type ReactElement, useState } from 'react';
import { toast } from 'sonner';

import { errorMessage, system } from '@/api';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
// Not the dialogs barrel: its launch dialog imports this directory back.
import { ExportInstanceDialog } from '@/features/instances/dialogs/export';
import { m } from '@/paraglide/messages.js';
import { instanceQueries } from '@/queries/instance';
import { usePinned } from '@/queries/pinned';
import { serverMutations, serverQueries } from '@/queries/server';
import { RemoveEntryDialog } from '../dialogs/remove';

export interface EntryMenuTarget {
  kind: 'instance' | 'server';
  id: string;
  name: string;
  running: boolean;
}

/**
 * The right-click menu every surface showing an entry shares. `children` is the
 * element that takes the right-click. The dialogs sit beside it rather than
 * inside: a portal still bubbles React events, and the trigger is a link.
 */
export function EntryContextMenu({
  entry,
  children,
}: {
  entry: EntryMenuTarget;
  children: ReactElement;
}) {
  const { kind, id } = entry;
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { ready, isPinned, toggle } = usePinned();
  const [removing, setRemoving] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const restart = useMutation(serverMutations.restart(id));
  const pinned = isPinned({ kind, id });

  const openFolder = () => {
    const info =
      kind === 'server'
        ? queryClient.fetchQuery(serverQueries.info(id))
        : queryClient.fetchQuery(instanceQueries.info(id));
    info
      .then(({ entryDir }) => system.openPath(entryDir))
      .catch((error: Error) => toast.error(errorMessage(error)));
  };

  const openSettings = () => {
    const search = { tab: 'settings' as const };
    if (kind === 'server') {
      navigate({ to: '/servers/$id', params: { id }, search });
    } else {
      navigate({ to: '/instances/$id', params: { id }, search });
    }
  };

  return (
    <ContextMenu>
      <ContextMenuTrigger render={children} />
      <ContextMenuContent className="w-44">
        <ContextMenuItem onClick={openFolder}>
          <FolderOpenIcon />
          {m['app.action.open_folder']()}
        </ContextMenuItem>
        <ContextMenuItem onClick={openSettings}>
          <GearSixIcon />
          {m['app.label.settings']()}
        </ContextMenuItem>
        {kind === 'instance' && (
          <ContextMenuItem onClick={() => setExporting(true)}>
            <UploadSimpleIcon />
            {m['app.action.export']()}
          </ContextMenuItem>
        )}
        <ContextMenuItem disabled={!ready} onClick={() => toggle({ kind, id })}>
          {pinned ? <PushPinSlashIcon /> : <PushPinIcon />}
          {pinned ? m['app.label.unpin']() : m['app.label.pin']()}
        </ContextMenuItem>
        {kind === 'server' && entry.running && (
          <ContextMenuItem
            disabled={restart.isPending}
            onClick={() => setRestarting(true)}
          >
            <ArrowClockwiseIcon />
            {m['app.action.restart']()}
          </ContextMenuItem>
        )}
        <ContextMenuSeparator />
        <ContextMenuItem
          variant="destructive"
          onClick={() => setRemoving(true)}
        >
          <TrashIcon />
          {m['app.action.delete']()}
        </ContextMenuItem>
      </ContextMenuContent>
      <RemoveEntryDialog
        kind={kind}
        id={id}
        name={entry.name}
        running={entry.running}
        open={removing}
        onOpenChange={setRemoving}
      />
      {kind === 'server' && (
        <ConfirmDialog
          open={restarting}
          onOpenChange={setRestarting}
          title={m['entry.restart.title']({ name: entry.name })}
          description={m['entry.restart.server_description']()}
          confirmLabel={m['app.action.restart']()}
          onConfirm={() => {
            restart.mutate();
            setRestarting(false);
          }}
        />
      )}
      {kind === 'instance' && (
        <ExportInstanceDialog
          id={id}
          name={entry.name}
          open={exporting}
          onOpenChange={setExporting}
        />
      )}
    </ContextMenu>
  );
}
