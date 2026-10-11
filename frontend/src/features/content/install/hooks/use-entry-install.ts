import { useState } from 'react';

import { dialog } from '@/api';
import { useDropzone } from '@/hooks/use-dropzone';
import { m } from '@/paraglide/messages.js';

/**
 * The install dialog's state as an entry page holds it: opened empty from the
 * add button, or on the files dropped onto the element `getRootProps` is
 * spread on — the entry's content tab.
 */
export function useEntryInstall(name: string) {
  const [open, setOpen] = useState(false);
  const [files, setFiles] = useState<string[]>([]);

  const { getRootProps } = useDropzone<HTMLDivElement>({
    accept: dialog.CONTENT_EXTENSIONS,
    label: m['content.drop']({ name }),
    onDrop: (paths) => {
      setFiles(paths);
      setOpen(true);
    },
  });

  return {
    open,
    files,
    getRootProps,
    onOpenChange: setOpen,
    start: () => {
      setFiles([]);
      setOpen(true);
    },
  };
}
