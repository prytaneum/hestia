/** `plugin.*` — query/mutation factories, consumed through useQuery/useMutation. */
import { queryOptions } from '@tanstack/react-query';
import { m } from '@/paraglide/messages.js';
import type {
  Effect,
  EntryKind,
  OutdatedPlugin,
  PluginInfo,
  PluginInspection,
  PluginInstallDone,
  PluginInstallProgress,
  PluginRunDone,
  PluginRunProgress,
  PluginUiActionParams,
  PluginUpdateDone,
  RepositoryAdded,
  RepositoryRefreshed,
} from '../api';
import * as api from '../api/plugin';
import { CATALOG_STALE_MS, mutation } from './core';
import { jobMutation } from './jobs';
import { keys } from './keys';

export type PluginInstallVariables = {
  id: string;
  approve: string;
} & api.PluginInstallTarget;

export type PluginUpdateVariables = { plugin: string; approve: string };

export type PluginRunVariables = {
  plugin: string;
  command: string;
  title: string;
  entry?: { kind: EntryKind; id: string };
  args: Record<string, unknown>;
};

export const pluginQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.plugins.list(),
      queryFn: () => api.list(),
    }),
  repositories: () =>
    queryOptions({
      queryKey: keys.plugins.repositories(),
      queryFn: () => api.repoList(),
    }),
  /** Reaches every repository, so it is cached like the other upstream catalogues; filtering is the caller's. */
  catalogue: () =>
    queryOptions({
      queryKey: keys.plugins.catalogue(),
      queryFn: () => api.catalogue(),
      staleTime: CATALOG_STALE_MS,
    }),
  /** Refreshes every repository like the catalogue; the daemon also pushes `plugin.updates`. */
  outdated: () =>
    queryOptions({
      queryKey: keys.plugins.outdated(),
      queryFn: () => api.outdated(),
      staleTime: CATALOG_STALE_MS,
    }),
  themes: () =>
    queryOptions({
      queryKey: keys.plugins.themes(),
      queryFn: () => api.themes(),
    }),
  pages: () =>
    queryOptions({
      queryKey: keys.plugins.pages(),
      queryFn: () => api.pages(),
    }),
  commands: () =>
    queryOptions({
      queryKey: keys.plugins.commands(),
      queryFn: () => api.commands(),
    }),
  settings: (id: string) =>
    queryOptions({
      queryKey: keys.plugins.settings(id),
      queryFn: () => api.settings(id),
    }),
  render: (plugin: string, page: string, entry?: string) =>
    queryOptions({
      queryKey: keys.plugins.render(plugin, page, entry),
      queryFn: () =>
        api.render({ plugin, page, context: entry ? { entry } : {} }),
      retry: false,
    }),
};

export const pluginMutations = {
  uiAction: () =>
    mutation<Effect[], PluginUiActionParams>({
      mutationKey: [...keys.plugins.all, 'ui-action'],
      mutationFn: (params) => api.action(params),
    }),
  setSetting: (id: string) =>
    mutation<void, { key: string; value: unknown }>({
      mutationKey: [...keys.plugins.settings(id), 'set'],
      mutationFn: ({ key, value }) => api.setSetting(id, key, value),
      invalidates: () => [keys.plugins.settings(id)],
    }),
  enable: () =>
    mutation<PluginInfo, string>({
      mutationKey: [...keys.plugins.all, 'enable'],
      mutationFn: (id) => api.enable(id),
      invalidates: () => [keys.plugins.all],
    }),
  disable: () =>
    mutation<PluginInfo, string>({
      mutationKey: [...keys.plugins.all, 'disable'],
      mutationFn: (id) => api.disable(id),
      invalidates: () => [keys.plugins.all],
    }),
  remove: () =>
    mutation<void, string>({
      mutationKey: [...keys.plugins.all, 'remove'],
      mutationFn: (id) => api.remove(id),
      invalidates: () => [keys.plugins.all],
    }),
  addRepository: () =>
    mutation<RepositoryAdded, { url: string; confirm: boolean }>({
      mutationKey: [...keys.plugins.all, 'repo-add'],
      mutationFn: ({ url, confirm }) => api.repoAdd(url, confirm),
      invalidates: ({ confirm }) => (confirm ? [keys.plugins.all] : []),
    }),
  removeRepository: () =>
    mutation<void, string>({
      mutationKey: [...keys.plugins.all, 'repo-remove'],
      mutationFn: (url) => api.repoRemove(url),
      invalidates: () => [keys.plugins.all],
    }),
  refreshRepositories: () =>
    mutation<RepositoryRefreshed, string | undefined>({
      mutationKey: [...keys.plugins.all, 'repo-refresh'],
      mutationFn: (url) => api.repoRefresh(url),
      invalidates: () => [keys.plugins.all],
    }),
  inspect: () =>
    mutation<PluginInspection, api.PluginPackage>({
      mutationKey: [...keys.plugins.all, 'inspect'],
      mutationFn: (source) => api.inspect(source),
    }),
  /** The one unsigned link install's offer, if its link now serves a newer version; asked on a click, never scheduled. */
  checkLink: () =>
    mutation<OutdatedPlugin | null, string>({
      mutationKey: [...keys.plugins.all, 'check-link'],
      mutationFn: async (id) =>
        (await api.outdated(true)).plugins.find((offer) => offer.id === id) ??
        null,
    }),
  /** A command's output is the plugin's own; text is shown, anything else only confirms. */
  run: () =>
    jobMutation<PluginRunDone, PluginRunVariables, PluginRunProgress>({
      mutationKey: [...keys.plugins.all, 'run'],
      meta: ({ entry }) => ({ channel: 'plugin.run', subject: entry }),
      run: ({ plugin, command, entry, args }, job) =>
        api.run({ plugin, command, entry: entry?.id, args }, job),
      invalidates: () => [],
      notify: {
        done: (done, { title }) => ({
          title: m['plugins.command.done']({ title }),
          description:
            typeof done.output === 'string' && done.output
              ? done.output
              : undefined,
        }),
      },
    }),
  install: () =>
    jobMutation<
      PluginInstallDone,
      PluginInstallVariables,
      PluginInstallProgress
    >({
      mutationKey: [...keys.plugins.all, 'install'],
      meta: () => ({ channel: 'plugin.install' }),
      run: (variables, job) => api.install(variables, job),
      invalidates: () => [keys.plugins.all],
      notify: {
        done: (done) => ({
          title: m['settings.plugins.installed']({
            name: done.plugin.manifest.name || done.plugin.manifest.id,
          }),
          warnings: done.warnings,
        }),
      },
    }),
  update: () =>
    jobMutation<PluginUpdateDone, PluginUpdateVariables, PluginInstallProgress>(
      {
        mutationKey: [...keys.plugins.all, 'update'],
        meta: () => ({ channel: 'plugin.update' }),
        run: (variables, job) => api.update(variables, job),
        invalidates: () => [keys.plugins.all],
        notify: {
          done: (done) => {
            const name = done.plugin.manifest.name || done.plugin.manifest.id;
            const rolledBack = done.warnings.some(
              (warning) => warning.kind === 'plugin_update_rolled_back',
            );
            return {
              title: rolledBack
                ? m['settings.plugins.update_rolled_back']({ name })
                : m['settings.plugins.updated']({
                    name,
                    version: done.plugin.manifest.version,
                  }),
              warnings: done.warnings,
            };
          },
        },
      },
    ),
};
