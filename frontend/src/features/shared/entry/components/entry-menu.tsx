import {
  ArrowClockwiseIcon,
  FolderOpenIcon,
  GearSixIcon,
  PushPinIcon,
  PushPinSlashIcon,
  TrashIcon,
  UploadSimpleIcon,
} from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { type ReactElement, useState } from 'react';

import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import { m } from '@/paraglide/messages.js';
import { serverMutations } from '@/queries/server';
import { type EntryActionTarget, useEntryActions } from '../hooks';

/**
 * The right-click menu every surface showing an entry shares. `children` is the
 * element that takes the right-click. The dialogs sit beside it rather than
 * inside: a portal still bubbles React events, and the trigger is a link.
 */
export function EntryContextMenu({
  entry,
  children,
}: {
  entry: EntryActionTarget;
  children: ReactElement;
}) {
  const { kind, id } = entry;
  const navigate = useNavigate();
  const actions = useEntryActions(entry);
  const [restarting, setRestarting] = useState(false);
  const restart = useMutation(serverMutations.restart(id));

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
        <ContextMenuItem onClick={actions.openFolder}>
          <FolderOpenIcon />
          {m['app.action.open_folder']()}
        </ContextMenuItem>
        <ContextMenuItem onClick={openSettings}>
          <GearSixIcon />
          {m['app.label.settings']()}
        </ContextMenuItem>
        {actions.exportArchive && (
          <ContextMenuItem onClick={actions.exportArchive}>
            <UploadSimpleIcon />
            {m['app.action.export']()}
          </ContextMenuItem>
        )}
        <ContextMenuItem
          disabled={!actions.pinReady}
          onClick={actions.togglePin}
        >
          {actions.pinned ? <PushPinSlashIcon /> : <PushPinIcon />}
          {actions.pinned ? m['app.label.unpin']() : m['app.label.pin']()}
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
        <ContextMenuItem variant="destructive" onClick={actions.remove}>
          <TrashIcon />
          {m['app.action.delete']()}
        </ContextMenuItem>
      </ContextMenuContent>
      {actions.dialogs}
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
    </ContextMenu>
  );
}
