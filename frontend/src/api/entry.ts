/** The `entry.*` channels: what a server and an instance share, aimed by kind or target. */
import { call } from './core/ipc';
import type { EntryRenamed } from './types/entry';
import type { EntryKind, EntryTarget } from './types/error';
import type {
  ConfigEntry,
  FlavorsResult,
  GameVersion,
} from './types/minecraft';
import type { ProcessLogLine } from './types/process';

export function flavors(kind: EntryKind): Promise<FlavorsResult> {
  return call('entry.flavors', { kind });
}

export function versions(
  kind: EntryKind,
  flavor: string,
): Promise<GameVersion[]> {
  return call('entry.versions', { kind, flavor });
}

/** Loader builds for a flavor/version, newest first; empty for vanilla. */
export function loaders(
  kind: EntryKind,
  flavor: string,
  version: string,
): Promise<string[]> {
  return call('entry.loaders', { kind, flavor, version });
}

export function rename(
  target: EntryTarget,
  name: string,
): Promise<EntryRenamed> {
  return call('entry.rename', { target, name });
}

export async function remove(target: EntryTarget): Promise<void> {
  await call('entry.remove', target);
}

/** An instance's one session when named, all of its sessions otherwise. */
export async function stop(
  target: EntryTarget,
  session?: string,
): Promise<void> {
  await call('entry.stop', { target, session });
}

/** An instance's newest running session unless one is named. */
export function logs(
  target: EntryTarget,
  options: { session?: string; tail?: number } = {},
): Promise<ProcessLogLine[]> {
  return call('entry.logs', {
    target,
    session: options.session,
    tail: options.tail,
  });
}

/** `memory` and `jvm-args`, plus any `server.properties` key on a server. */
export const config = {
  async set(target: EntryTarget, key: string, value: string): Promise<void> {
    await call('entry.config.set', { target, key, value });
  },

  async list(target: EntryTarget): Promise<ConfigEntry[]> {
    const result = await call('entry.config.list', target);
    return result.entries;
  },
};
