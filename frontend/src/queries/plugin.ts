/** `plugin.*` — query/mutation factories, consumed through useQuery/useMutation. */
import { queryOptions } from '@tanstack/react-query';
import { m } from '@/paraglide/messages.js';
import type {
  PluginInfo,
  PluginInspection,
  PluginInstallDone,
  PluginInstallProgress,
} from '../api';
import * as api from '../api/plugin';
import { mutation } from './core';
import { jobMutation } from './jobs';
import { keys } from './keys';

export type PluginInstallVariables = {
  id: string;
  approve: string;
} & api.PluginInstallTarget;

export const pluginQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.plugins.list(),
      queryFn: () => api.list(),
    }),
};

export const pluginMutations = {
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
  inspect: () =>
    mutation<PluginInspection, api.PluginPackage>({
      mutationKey: [...keys.plugins.all, 'inspect'],
      mutationFn: (source) => api.inspect(source),
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
        }),
      },
    }),
};
