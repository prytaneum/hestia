/**
 * Native file dialogs, over `@tauri-apps/plugin-dialog`. A local-file content
 * import needs a real daemon-readable path (the daemon reads the file itself),
 * which only the shell's native picker can produce — a browser file input hands
 * back a sandboxed `File`, never a path.
 */
import { open, save } from '@tauri-apps/plugin-dialog';

/** Single-file content is a jar or zip; a `.mrpack` is a modpack, not offered here. */
export const CONTENT_EXTENSIONS = ['jar', 'zip'];

/**
 * Pick content files and return their absolute paths (empty if the dialog was
 * dismissed). Each path is passed straight to `content.add`'s `path` field.
 */
export async function pickContentFiles(): Promise<string[]> {
  const selection = await open({
    multiple: true,
    directory: false,
    filters: [{ name: 'Content', extensions: CONTENT_EXTENSIONS }],
  });
  if (Array.isArray(selection)) return selection;
  return typeof selection === 'string' ? [selection] : [];
}

/** The image formats an entry icon accepts, by extension. */
export const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'webp', 'gif'];

/** Pick one image and return its absolute path, or null when dismissed. */
export async function pickImage(): Promise<string | null> {
  const selection = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Images', extensions: IMAGE_EXTENSIONS }],
  });
  return typeof selection === 'string' ? selection : null;
}

/** The archive formats an instance import accepts, by extension. */
export const ARCHIVE_EXTENSIONS = ['hestia', 'mrpack', 'zip'];

/** Pick one instance archive to import; empty when dismissed. */
export async function pickInstanceArchive(): Promise<string[]> {
  const selection = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Instance archives', extensions: ARCHIVE_EXTENSIONS }],
  });
  return typeof selection === 'string' ? [selection] : [];
}

/**
 * Ask where to write an export. Returns an absolute path, or null when
 * dismissed — the daemon writes it, so only a real filesystem path will do.
 */
export function pickExportPath(
  suggestedName: string,
  extension: string,
): Promise<string | null> {
  return save({
    defaultPath: suggestedName,
    filters: [{ name: 'Archive', extensions: [extension] }],
  });
}
