/**
 * Host-shell affordances that do not cross the daemon socket — opening paths
 * and links with the host's default handler.
 */

import { invokeCommand } from './core/ipc';

/** Open a folder (or file) in the OS file manager. */
export function openPath(path: string): Promise<void> {
  return invokeCommand('open_path', { path });
}

/**
 * Show a file in the OS file manager, selected in its folder — what a `.jar`
 * needs, since opening it would hand it to whatever claims the extension.
 */
export function revealPath(path: string): Promise<void> {
  return invokeCommand('plugin:opener|reveal_item_in_dir', { paths: [path] });
}

/**
 * The instance archive the app was launched with (double-clicking a `.hestia`
 * file), cleared as it is read. Null on an ordinary start.
 */
export function pendingArchive(): Promise<string | null> {
  return invokeCommand('pending_archive');
}

/** Open a URL in the user's default browser (markdown links, project pages). */
export function openUrl(url: string): Promise<void> {
  return invokeCommand('open_url', { url });
}
