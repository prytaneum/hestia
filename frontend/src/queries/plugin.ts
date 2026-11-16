/** `plugin.*` — queries, and `usePluginNamer` over them. */
import { queryOptions, useQuery } from '@tanstack/react-query';
import * as api from '../api/plugin';
import { keys } from './keys';

export const pluginQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.plugins.list(),
      queryFn: () => api.list(),
    }),
};

/** The name the user approved for a plugin id, or the id until it is known. */
export function usePluginNamer(): (id: string) => string {
  const { data } = useQuery(pluginQueries.list());
  return (id) => data?.find((plugin) => plugin.id === id)?.name || id;
}
