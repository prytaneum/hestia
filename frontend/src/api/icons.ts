/**
 * An entry's icon is read where the daemon keeps it, in the entry's own
 * directory, so the shell has to let the webview at each one before its
 * `asset:` URL resolves.
 */
import { convertFileSrc } from '@tauri-apps/api/core';
import { invokeCommand } from './core/ipc';
import type { EntryIcon } from './types/icon';

/** Let the webview load the entries' icons, then hand the entries back. */
export async function allowed<T extends { icon?: EntryIcon }>(
  entries: T[],
): Promise<T[]> {
  const paths = entries.flatMap((entry) =>
    entry.icon ? [entry.icon.path] : [],
  );
  if (paths.length > 0) await invokeCommand('icons_allow', { paths });
  return entries;
}

export function url(icon: EntryIcon): string {
  return `${convertFileSrc(icon.path)}?v=${icon.revision}`;
}

/** The entry with the URL its icon loads from, the shape every row renders. */
export function withUrl<T extends { icon?: EntryIcon }>(
  entry: T,
): T & { iconUrl?: string } {
  return { ...entry, iconUrl: entry.icon && url(entry.icon) };
}
