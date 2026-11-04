/** The `plugin.*` channels. */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type {
  PluginInfo,
  PluginInspection,
  PluginInstallDone,
} from './types/plugin';

export function list(): Promise<PluginInfo[]> {
  return call('plugin.list');
}

export function enable(id: string): Promise<PluginInfo> {
  return call('plugin.enable', { id });
}

export function disable(id: string): Promise<PluginInfo> {
  return call('plugin.disable', { id });
}

/** Deletes the plugin's data directory with it. */
export async function remove(id: string): Promise<void> {
  await call('plugin.remove', { id });
}

/** What installing the package at `path` would grant; runs none of it. */
export function inspect(path: string): Promise<PluginInspection> {
  return call('plugin.inspect', { path });
}

/** `approve` is the `grantDigest` of the inspection the user approved. */
export function install(
  params: { id: string; path: string; approve: string },
  job: JobRun,
): Promise<PluginInstallDone> {
  return runJob('plugin.install', params, job);
}
