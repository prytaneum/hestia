/** `server.players.*` — who is on a server and who may be. */
import { queryOptions } from '@tanstack/react-query';
import type { PlayerList } from '../api';
import { players as api } from '../api';
import { mutation } from './core';
import { keys } from './keys';

/** How often the online list is re-read while a server runs. */
const ONLINE_POLL_MS = 10_000;

export const playersQueries = {
  list: (id: string, running: boolean) =>
    queryOptions({
      queryKey: keys.servers.players(id),
      queryFn: () => api.list(id),
      refetchInterval: running ? ONLINE_POLL_MS : false,
    }),
};

export const playersMutations = {
  set: (id: string) =>
    mutation<void, { player: string; list: PlayerList; listed: boolean }>({
      mutationKey: [...keys.servers.players(id), 'set'],
      mutationFn: (v) => api.set(id, v.player, v.list, v.listed),
      invalidates: () => [keys.servers.players(id)],
    }),
  kick: (id: string) =>
    mutation<void, string>({
      mutationKey: [...keys.servers.players(id), 'kick'],
      mutationFn: (player) => api.kick(id, player),
      invalidates: () => [keys.servers.players(id)],
    }),
};
