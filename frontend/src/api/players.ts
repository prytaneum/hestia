/**
 * The `server.players.*` channels. The lists come from the server's own files
 * and answer for a stopped server; `set` and `kick` are console commands and
 * need it running.
 */
import { call } from './core/ipc';
import type { PlayerList, ServerPlayersResult } from './types/players';

export function list(server: string): Promise<ServerPlayersResult> {
  return call('server.players', { server });
}

/** Put `player` on `list`, or take them off it. */
export async function set(
  server: string,
  player: string,
  playerList: PlayerList,
  listed: boolean,
): Promise<void> {
  await call('server.players.set', {
    server,
    player,
    list: playerList,
    listed,
  });
}

export async function kick(server: string, player: string): Promise<void> {
  await call('server.players.kick', { server, player });
}
