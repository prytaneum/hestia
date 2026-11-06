/** The `plugin.*` channels. */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type {
  PluginInfo,
  PluginInspection,
  PluginInstallDone,
  PluginPage,
  PluginUiActionParams,
  PluginUiRenderParams,
} from './types/plugin';
import type { Effect, UiNode } from './types/ui';

export function pages(): Promise<PluginPage[]> {
  return call('plugin.pages');
}

export function render(params: PluginUiRenderParams): Promise<UiNode> {
  return call('plugin.ui.render', params);
}

export function action(params: PluginUiActionParams): Promise<Effect[]> {
  return call('plugin.ui.action', params);
}

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

/** A local file or folder, or an https link the daemon downloads once. */
export type PluginPackage = { path: string } | { url: string };

/** A local package by its path, or a downloaded one by its inspection's `staged` token. */
export type PluginInstallTarget = { path: string } | { staged: string };

/** What installing the package would grant; runs none of it. */
export function inspect(source: PluginPackage): Promise<PluginInspection> {
  return call('plugin.inspect', source);
}

/** `approve` is the `grantDigest` of the inspection the user approved. */
export function install(
  params: { id: string; approve: string } & PluginInstallTarget,
  job: JobRun,
): Promise<PluginInstallDone> {
  return runJob('plugin.install', params, job);
}
