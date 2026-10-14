import {
  DotsThreeIcon,
  FolderOpenIcon,
  TrashIcon,
  UploadSimpleIcon,
} from '@phosphor-icons/react';

import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { m } from '@/paraglide/messages.js';
import { type EntryActionTarget, useEntryActions } from '../hooks';

/**
 * The detail hero's overflow: the entry's less frequent actions, reachable
 * from every tab. Pin and run controls stay visible beside it.
 */
export function EntryActionsMenu({ entry }: { entry: EntryActionTarget }) {
  const actions = useEntryActions(entry);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="outline"
              size="icon"
              aria-label={m['app.action.more']()}
              title={m['app.action.more']()}
            >
              <DotsThreeIcon weight="bold" className="size-4" />
            </Button>
          }
        />
        <DropdownMenuContent align="end" className="w-44">
          <DropdownMenuItem onClick={actions.openFolder}>
            <FolderOpenIcon />
            {m['app.action.open_folder']()}
          </DropdownMenuItem>
          {actions.exportArchive && (
            <DropdownMenuItem onClick={actions.exportArchive}>
              <UploadSimpleIcon />
              {m['app.action.export']()}
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onClick={actions.remove}>
            <TrashIcon />
            {m['app.action.delete']()}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {actions.dialogs}
    </>
  );
}
