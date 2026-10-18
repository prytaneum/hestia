/**
 * Custom entry icons: desktop-local, copied into `<data_home>/icons/` by the
 * shell (`icons_*` commands) and served over the asset protocol. Keyed by the
 * entry's stable id; the disk is the registry.
 */
import { convertFileSrc } from '@tauri-apps/api/core';
import { invokeCommand } from './core/ipc';

/** One stored icon; `mtime` doubles as the cache-busting version. */
export interface IconEntry {
  path: string;
  mtime: number;
}

/** The generated-icon background, tagged by type; colors are `#rrggbb`. */
export type IconBackground =
  | { type: 'color'; value: string }
  | {
      type: 'linear-top-down-gradient';
      top_color: string;
      bottom_color: string;
    };

/** The sidecar config that produced a generated icon. */
export interface IconConfig {
  background: IconBackground;
  symbol: string;
}

export function list(): Promise<Record<string, IconEntry>> {
  return invokeCommand('icons_list');
}

/** Copy a picked image into the data home as `entryId`'s icon. */
export function set(entryId: string, sourcePath: string): Promise<IconEntry> {
  return invokeCommand('icon_set', { entryId, sourcePath });
}

export function fetch(entryId: string, url: string): Promise<IconEntry> {
  return invokeCommand('icon_fetch', { entryId, url });
}

/** Delete the entry's icon and generation config. */
export function remove(entryId: string): Promise<void> {
  return invokeCommand('icon_remove', { entryId });
}

/** Store the baked `bytes` as the entry's icon, recording `config` beside them. */
export function generate(
  entryId: string,
  config: IconConfig,
  bytes: number[],
): Promise<IconEntry> {
  return invokeCommand('icon_set_bytes', { entryId, config, bytes });
}

/** The stored generation config, or null when the icon was picked instead. */
export function config(entryId: string): Promise<IconConfig | null> {
  return invokeCommand('icon_config', { entryId });
}

/** The webview-loadable URL for a stored icon. */
export function iconUrl(entry: IconEntry): string {
  return `${convertFileSrc(entry.path)}?v=${entry.mtime}`;
}
