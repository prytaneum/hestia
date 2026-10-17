/**
 * The `server.files.*` channels: a server's game directory as a file tree.
 * Every path is relative to that directory and `/`-separated; the empty path
 * is its root. `importFiles` takes absolute paths on this machine — the daemon
 * is local, so an upload is a copy.
 */
import { call } from './core/ipc';
import type { FileEntry } from './types/files';

export function list(server: string, path: string): Promise<FileEntry[]> {
  return call('server.files.list', { server, path });
}

/** A text file whole; a binary or oversized one is refused. */
export async function read(server: string, path: string): Promise<string> {
  const result = await call('server.files.read', { server, path });
  return result.content;
}

export async function write(
  server: string,
  path: string,
  content: string,
): Promise<void> {
  await call('server.files.write', { server, path, content });
}

export async function mkdir(server: string, path: string): Promise<void> {
  await call('server.files.mkdir', { server, path });
}

/** Rename or move; never replaces what is already at `to`. */
export async function move(
  server: string,
  from: string,
  to: string,
): Promise<void> {
  await call('server.files.move', { server, from, to });
}

export async function copy(
  server: string,
  from: string,
  to: string,
): Promise<void> {
  await call('server.files.copy', { server, from, to });
}

export async function remove(server: string, paths: string[]): Promise<void> {
  await call('server.files.remove', { server, paths });
}

export async function importFiles(
  server: string,
  path: string,
  sources: string[],
): Promise<void> {
  await call('server.files.import', { server, path, sources });
}

export async function compress(
  server: string,
  paths: string[],
  to: string,
): Promise<void> {
  await call('server.files.compress', { server, paths, to });
}

export async function extract(
  server: string,
  from: string,
  to: string,
): Promise<void> {
  await call('server.files.extract', { server, from, to });
}
