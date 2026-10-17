/**
 * `screenshot.*` — every instance's screenshots as one listing. Nothing is
 * copied: a shot is read where the game wrote it, so the shell has to let the
 * webview into that folder before its `asset:` URL resolves.
 */
import { convertFileSrc } from '@tauri-apps/api/core';

import { call, invokeCommand } from './core/ipc';
import type { Screenshot } from './types/screenshot';

export async function list(instance = ''): Promise<Screenshot[]> {
  const result = await call('screenshot.list', { instance });
  return result.screenshots;
}

export async function remove(
  instance: string,
  file: string,
): Promise<Screenshot[]> {
  const result = await call('screenshot.delete', { instance, file });
  return result.screenshots;
}

export function allow(dirs: string[]): Promise<void> {
  return invokeCommand('screenshots_allow', { dirs });
}

export function folders(screenshots: Screenshot[]): string[] {
  const dirs = screenshots.map((shot) =>
    shot.path.slice(
      0,
      Math.max(shot.path.lastIndexOf('/'), shot.path.lastIndexOf('\\')),
    ),
  );
  return [...new Set(dirs.filter((dir) => dir.length > 0))];
}

export function url(shot: Screenshot): string {
  return `${convertFileSrc(shot.path)}?v=${shot.takenUnix}`;
}
