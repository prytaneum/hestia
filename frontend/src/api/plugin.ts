/** The `plugin.*` channels. */

import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type {
  Catalogue,
  RepositoryAdded,
  RepositoryInfo,
  RepositoryRefreshed,
} from './types/catalogue';
import type {
  Outdated,
  PluginCommand,
  PluginInfo,
  PluginInspection,
  PluginInstallDone,
  PluginPage,
  PluginRunDone,
  PluginTheme,
  PluginUiActionParams,
  PluginUiRenderParams,
  PluginUpdateDone,
  SettingValue,
} from './types/plugin';
import type { Effect, UiNode } from './types/ui';

export function themes(): Promise<PluginTheme[]> {
  return call('plugin.themes');
}

export function pages(): Promise<PluginPage[]> {
  return call('plugin.pages');
}

export function commands(): Promise<PluginCommand[]> {
  return call('plugin.commands');
}

/** Runs a plugin command; `entry` is the server or instance a scoped command acts on. */
export function run(
  params: {
    plugin: string;
    command: string;
    entry?: string;
    args: Record<string, unknown>;
  },
  job: JobRun,
): Promise<PluginRunDone> {
  return runJob('plugin.run', params, job);
}

export function render(params: PluginUiRenderParams): Promise<UiNode> {
  return call('plugin.ui.render', params);
}

export function action(params: PluginUiActionParams): Promise<Effect[]> {
  return call('plugin.ui.action', params);
}

/** Fetches and verifies the index; saves the repository only with `confirm`. */
export function repoAdd(
  url: string,
  confirm: boolean,
): Promise<RepositoryAdded> {
  return call('plugin.repo.add', { url, confirm });
}

export function repoList(): Promise<RepositoryInfo[]> {
  return call('plugin.repo.list');
}

/** Plugins installed from it keep running, as obsolete. */
export async function repoRemove(url: string): Promise<void> {
  await call('plugin.repo.remove', { url });
}

/** One repository by its index URL, or every configured one. */
export function repoRefresh(url?: string): Promise<RepositoryRefreshed> {
  return call('plugin.repo.refresh', { url });
}

/** Every plugin the configured repositories offer; unreachable ones answer from their cached index. */
export function catalogue(): Promise<Catalogue> {
  return call('plugin.catalogue');
}

/** `unsigned` also re-downloads unsigned link installs to read their version. */
export function outdated(unsigned = false): Promise<Outdated> {
  return call('plugin.outdated', { unsigned });
}

/**
 * Moves an installed plugin to its newest build. `approve` is the `grantDigest`
 * of the update's inspection; the daemon refuses an update that asks for more
 * than is installed without it.
 */
export function update(
  params: { plugin: string; approve: string },
  job: JobRun,
): Promise<PluginUpdateDone> {
  return runJob('plugin.update', params, job);
}

export function list(): Promise<PluginInfo[]> {
  return call('plugin.list');
}

/** A secret reports only whether it is set, never its value. */
export function settings(id: string): Promise<SettingValue[]> {
  return call('plugin.settings.get', { id });
}

/** `null` clears the setting back to its default; the daemon checks the value against the declared kind. */
export async function setSetting(
  id: string,
  key: string,
  value: unknown,
): Promise<void> {
  await call('plugin.settings.set', { id, key, value });
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

/**
 * A local file or folder, an https link the daemon downloads once, or a plugin
 * of a configured repository (`repo` is its index URL): the daemon downloads
 * the newest compatible build and checks it against the repository's key.
 */
export type PluginPackage =
  | { path: string }
  | { url: string }
  | { repo: string; plugin: string };

/** A local package by its path, or a downloaded one by its inspection's `staged` token. */
export type PluginInstallTarget =
  | { path: string; dev?: true }
  | { staged: string };

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
