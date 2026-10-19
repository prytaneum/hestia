/** `java.*` — query/mutation factories, consumed through useQuery/useMutation. */
import { queryOptions } from '@tanstack/react-query';
import type { JavaInstallDone, JavaInstallProgress } from '../api';
import * as api from '../api/java';
import { CATALOG_STALE_MS, mutation } from './core';
import { jobMutation } from './jobs';
import { keys } from './keys';

export const javaQueries = {
  releases: () =>
    queryOptions({
      queryKey: keys.java.releases(),
      queryFn: () => api.releases(),
      staleTime: CATALOG_STALE_MS,
    }),
  runtimes: () =>
    queryOptions({
      queryKey: keys.java.runtimes(),
      queryFn: () => api.list(),
    }),
};

export const javaMutations = {
  install: () =>
    jobMutation<
      JavaInstallDone,
      { major: number; force?: boolean },
      JavaInstallProgress
    >({
      mutationKey: [...keys.java.all, 'install'],
      meta: () => ({ channel: 'java.install' }),
      run: ({ major, force }, job) => api.install(major, { force }, job),
      invalidates: () => [keys.java.all],
    }),
  uninstall: () =>
    mutation<void, number>({
      mutationKey: [...keys.java.all, 'uninstall'],
      mutationFn: (major) => api.uninstall(major),
      invalidates: () => [keys.java.all],
    }),
};
