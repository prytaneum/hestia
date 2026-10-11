import { useState } from 'react';

import { dialog } from '@/api';
import { useDropTarget } from '@/hooks/use-drop-target';
import { m } from '@/paraglide/messages.js';

/**
 * The install dialog's state as an entry page holds it: opened empty from the
 * add button, or on the files dropped onto the page. `droppable` is whether the
 * page is showing the surface a drop belongs to — its content tab.
 */
export function useEntryInstall(name: string, droppable: boolean) {
  const [open, setOpen] = useState(false);
  const [files, setFiles] = useState<string[]>([]);

  // Closed only: an open dialog has its own drop zone, and a drop it does not
  // take must not fall through and restart it.
  useDropTarget({
    extensions: dialog.CONTENT_EXTENSIONS,
    label: m['content.drop']({ name }),
    enabled: droppable && !open,
    onDrop: (paths) => {
      setFiles(paths);
      setOpen(true);
    },
  });

  return {
    open,
    files,
    onOpenChange: setOpen,
    start: () => {
      setFiles([]);
      setOpen(true);
    },
  };
}
