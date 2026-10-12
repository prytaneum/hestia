/** `server.task.*` — what a server runs on a schedule. */
import { queryOptions } from '@tanstack/react-query';
import type { ServerTask } from '../api';
import { tasks as api } from '../api';
import { mutation } from './core';
import { keys } from './keys';

export const tasksQueries = {
  list: (id: string) =>
    queryOptions({
      queryKey: keys.servers.tasks(id),
      queryFn: () => api.list(id),
    }),
};

export const tasksMutations = {
  save: (id: string) =>
    mutation<ServerTask, Partial<ServerTask>>({
      mutationKey: [...keys.servers.tasks(id), 'save'],
      mutationFn: (task) => api.save(id, task),
      invalidates: () => [keys.servers.tasks(id)],
    }),
  remove: (id: string) =>
    mutation<void, string>({
      mutationKey: [...keys.servers.tasks(id), 'remove'],
      mutationFn: (task) => api.remove(id, task),
      invalidates: () => [keys.servers.tasks(id)],
    }),
};
