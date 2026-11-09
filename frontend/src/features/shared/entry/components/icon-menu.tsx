/**
 * The hero-icon overlay button: opens the shared icon editor directly, where
 * a custom image, the generated pickers, and reset all live.
 */
import { PencilSimpleIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import type { EntryIcon, EntryKind } from '@/api';
import { m } from '@/paraglide/messages.js';
import { IconEditorDialog } from './icon-editor/editor-dialog';

export function EntryIconMenu({
  kind,
  id,
  icon,
}: {
  kind: EntryKind;
  id: string;
  icon?: EntryIcon;
}) {
  const [editorOpen, setEditorOpen] = useState(false);

  return (
    <>
      <button
        type="button"
        aria-label={m['entry.icon.customize']()}
        onClick={() => setEditorOpen(true)}
        className="grid size-5 place-items-center bg-background/80 text-muted-foreground ring-1 ring-border backdrop-blur-xs outline-none hover:text-foreground focus-visible:ring-ring"
      >
        <PencilSimpleIcon className="size-3" />
      </button>
      <IconEditorDialog
        open={editorOpen}
        onOpenChange={setEditorOpen}
        kind={kind}
        id={id}
        icon={icon}
      />
    </>
  );
}
