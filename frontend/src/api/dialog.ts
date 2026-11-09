/**
 * Native file dialogs, over `@tauri-apps/plugin-dialog`. A local-file content
 * import needs a real daemon-readable path (the daemon reads the file itself),
 * which only the shell's native picker can produce — a browser file input hands
 * back a sandboxed `File`, never a path.
 */
import { open, save } from '@tauri-apps/plugin-dialog';

import type { ContentKind } from './types';

/** Single-file content is a jar or zip; a `.mrpack` is a modpack, not offered here. */
const CONTENT_EXTENSIONS = ['jar', 'zip'];
const PACK_EXTENSIONS = ['mcpack', 'mcaddon'];

/** Local-file extensions an entry takes; a Bedrock pack ships as `.mcpack`/`.mcaddon`. */
export const contentExtensions = (accepts: ContentKind[]) =>
  accepts.includes('behavior_pack')
    ? [...CONTENT_EXTENSIONS, ...PACK_EXTENSIONS]
    : CONTENT_EXTENSIONS;

const named = (name: string, extensions?: string[]) =>
  extensions && [{ name, extensions }];

async function pickOne(
  name: string,
  extensions?: string[],
): Promise<string | null> {
  const selection = await open({
    multiple: false,
    directory: false,
    filters: named(name, extensions),
  });
  return typeof selection === 'string' ? selection : null;
}

async function pickMany(
  name: string,
  extensions?: string[],
): Promise<string[]> {
  const selection = await open({
    multiple: true,
    directory: false,
    filters: named(name, extensions),
  });
  if (Array.isArray(selection)) return selection;
  return typeof selection === 'string' ? [selection] : [];
}

/** Pick content files and return their absolute paths (empty if dismissed). */
export const pickContentFiles = (accepts: ContentKind[]) =>
  pickMany('Content', contentExtensions(accepts));

/** Pick one content file to update a local item from, or null when dismissed. */
export const pickContentFile = (accepts: ContentKind[]) =>
  pickOne('Content', contentExtensions(accepts));

export const pickWorldFile = () => pickOne('World', ['mcworld']);

/** Pick any files to copy into a server's directory; empty when dismissed. */
export const pickFiles = () => pickMany('Files');

/** The image formats an entry icon accepts, by extension. */
export const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'webp', 'gif'];

/** Pick one image and return its absolute path, or null when dismissed. */
export const pickImage = () => pickOne('Images', IMAGE_EXTENSIONS);

/** The archive formats an instance import accepts, by extension. */
export const ARCHIVE_EXTENSIONS = ['hestia', 'mrpack', 'zip'];

/** Pick one instance archive to import; empty when dismissed. */
export async function pickInstanceArchive(): Promise<string[]> {
  const path = await pickOne('Instance archives', ARCHIVE_EXTENSIONS);
  return path ? [path] : [];
}

/** Pick one modpack file and return its absolute path, or null when dismissed. */
export const pickPackFile = () => pickOne('Modpacks', ['mrpack', 'zip']);

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
