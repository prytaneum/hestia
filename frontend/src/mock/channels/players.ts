/**
 * `server.players.*`. One set of lists per server; a change is refused for a
 * stopped server, as the daemon refuses it.
 */
import type { PlayerEntry, PlayerList } from '@/api/types';

import * as processes from '../state/processes';
import { bool, type ChannelHandlers, fail, ok, str } from '../support';

type Lists = Record<PlayerList, PlayerEntry[]>;

const seed = (): Lists => ({
  whitelist: [
    { name: 'Alex', uuid: '6ab43178-89fd-4905-97f6-0f67d9d76fd9' },
    { name: 'Steve', uuid: '8667ba71-b85a-4004-af54-457a9734eed7' },
  ],
  ops: [{ name: 'Alex', uuid: '6ab43178-89fd-4905-97f6-0f67d9d76fd9' }],
  banned: [
    {
      name: 'Herobrine',
      uuid: 'f84c6a79-0a4e-45e0-879b-cd49ebd4c4e2',
      reason: 'griefing',
    },
  ],
});

const lists = new Map<string, Lists>();

function listsOf(server: string): Lists {
  let mine = lists.get(server);
  if (!mine) {
    mine = seed();
    lists.set(server, mine);
  }
  return mine;
}

const running = (server: string): boolean =>
  processes.isRunning(processes.serverProcessId(server));

function requireRunning(server: string): void {
  if (!running(server)) fail('bad_request', `${server} is not running`);
}

export const channels: ChannelHandlers = {
  'server.players': (p) => {
    const server = str(p, 'server');
    return {
      online: running(server)
        ? [{ name: 'Alex', uuid: '6ab43178-89fd-4905-97f6-0f67d9d76fd9' }]
        : [],
      ...listsOf(server),
    };
  },

  'server.players.set': (p) => {
    const server = str(p, 'server');
    requireRunning(server);
    const player = str(p, 'player');
    const list = listsOf(server)[str(p, 'list') as PlayerList];
    const at = list.findIndex((entry) => entry.name === player);
    if (bool(p, 'listed')) {
      if (at < 0) list.push({ name: player, uuid: crypto.randomUUID() });
    } else if (at >= 0) {
      list.splice(at, 1);
    }
    return ok();
  },

  'server.players.kick': (p) => {
    requireRunning(str(p, 'server'));
    return ok();
  },
};
