import { dialog } from '@/api';
import { useFileDrop } from '@/hooks/use-file-drop';

export function useArchiveDrop(onDrop: (path: string) => void): boolean {
  return useFileDrop(dialog.ARCHIVE_EXTENSIONS, ([archive]) => onDrop(archive));
}
