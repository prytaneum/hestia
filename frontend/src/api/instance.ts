/**
 * The `instance.*` channels. Unlike a server, an instance is a lightweight
 * record at create — its files materialise during the `launch` job. An
 * instance can run several concurrent sessions (opt-in via `new_session`);
 * `stop` and `logs` take an optional session id.
 */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type {
  InstanceCreateParams,
  InstanceDetails,
  InstanceInfo,
  InstanceLaunchDone,
  InstanceLaunchParams,
  InstanceServerEditParams,
  InstanceServersWriteResult,
  InstanceUpdateDone,
  InstanceUpdateParams,
  RecentPlay,
  ServerEntry,
  WorldInfo,
} from './types/instance';
import type {
  ConfigEntry,
  FlavorsResult,
  GameVersion,
} from './types/minecraft';
import type { ProcessLogLine } from './types/process';
import type { ServerPingResult } from './types/server';

export function flavors(): Promise<FlavorsResult> {
  return call('instance.flavors');
}

export function versions(flavor: string): Promise<GameVersion[]> {
  return call('instance.versions', { flavor });
}

/** Loader builds for a flavor/version, newest first; empty for vanilla. */
export function loaders(flavor: string, version: string): Promise<string[]> {
  return call('instance.loaders', { flavor, version });
}

export function list(): Promise<InstanceInfo[]> {
  return call('instance.list');
}

/** The instance's static, informational view (locations + disk footprint). */
export function info(instance: string): Promise<InstanceDetails> {
  return call('instance.info', { instance });
}

export function create(
  params: Partial<InstanceCreateParams>,
): Promise<InstanceInfo> {
  return call('instance.create', params);
}

/** The instance pays for the new version at its next launch. */
export function update(
  params: Omit<InstanceUpdateParams, 'id'>,
  job: JobRun,
): Promise<InstanceUpdateDone> {
  return runJob('instance.update', params, job);
}

export function rename(instance: string, name: string): Promise<InstanceInfo> {
  return call('instance.rename', { instance, name });
}

export async function remove(instance: string): Promise<void> {
  await call('instance.remove', { instance });
}

/** The save worlds, each described from its own `level.dat`. */
export function worlds(instance: string): Promise<WorldInfo[]> {
  return call('instance.worlds', { instance });
}

/**
 * What was joined lately across every instance, most recent first — each one a
 * launch of `instance` with `target` as its quick play.
 */
export function recent(): Promise<RecentPlay[]> {
  return call('instance.recent');
}

/** The multiplayer list (`servers.dat`), in the order the game shows it. */
export function servers(instance: string): Promise<ServerEntry[]> {
  return call('instance.servers', { instance });
}

/**
 * Add an entry to the multiplayer list, or rewrite the one `server` names
 * (empty adds). The running game owns the file, so the result carries a
 * warning when a session is open to say the edit will be overwritten.
 */
export function serverEdit(
  params: Partial<InstanceServerEditParams>,
): Promise<InstanceServersWriteResult> {
  return call('instance.server.edit', params);
}

export function serverRemove(
  instance: string,
  server: string,
): Promise<InstanceServersWriteResult> {
  return call('instance.server.remove', { instance, server });
}

/**
 * Rewrite the order of the multiplayer list. `order` names every visible entry
 * — the rows that were listed — in the order it should sit.
 */
export function serversArrange(
  instance: string,
  order: string[],
): Promise<InstanceServersWriteResult> {
  return call('instance.servers.arrange', { instance, order });
}

/** Status of a multiplayer address, over the Server List Ping. */
export function pingAddress(address: string): Promise<ServerPingResult> {
  return call('minecraft.ping', { address });
}

/**
 * Materialise the instance's files and spawn the game as the signed-in
 * account. Resolves once the session is running.
 */
export function launch(
  params: Partial<InstanceLaunchParams>,
  job: JobRun,
): Promise<InstanceLaunchDone> {
  return runJob('instance.launch', params, job);
}

/** Stops one named session, or every session of the instance. */
export async function stop(instance: string, session?: string): Promise<void> {
  await call('instance.stop', { instance, session });
}

/** Targets the newest running session unless one is named. */
export function logs(
  instance: string,
  options: { session?: string; tail?: number } = {},
): Promise<ProcessLogLine[]> {
  return call('instance.logs', {
    instance,
    session: options.session,
    tail: options.tail,
  });
}

/** `memory` and `jvm-args` only. */
export const config = {
  async set(instance: string, key: string, value: string): Promise<void> {
    await call('instance.config.set', { instance, key, value });
  },

  async list(instance: string): Promise<ConfigEntry[]> {
    const result = await call('instance.config.list', { instance });
    return result.entries;
  },
};
