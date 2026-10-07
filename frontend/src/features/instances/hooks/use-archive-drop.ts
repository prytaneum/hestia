import { dialog } from '@/api';
import { useFileDrop } from '@/hooks/use-file-drop';

/** Dropping an instance archive onto a page; the first one dropped wins. */
export function useArchiveDrop(onDrop: (path: string) => void): boolean {
  return useFileDrop(dialog.ARCHIVE_EXTENSIONS, ([archive]) => onDrop(archive));
}
