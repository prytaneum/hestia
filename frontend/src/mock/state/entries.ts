/**
 * The entry store: the instance and server records the fixture daemon owns,
 * plus the per-entry settings and the pack an entry was built from. Every
 * reference resolves by id *or* display name, as `proto::naming` does, so a
 * front-end that names an entry either way is answered either way.
 *
 * Live process state is not held here — a record is composed with it at read
 * time (see ./processes), which keeps the two stores acyclic.
 */
import type {
  ConfigEntry,
  InstalledModpack,
  InstanceInfo,
  PlayerList,
  ServerInfo,
  WorldInfo,
} from '@/api/types';

import { ago, fail, now, slug } from '../support';
import { accepts, javaFor } from './catalog';

/** The data home every path the fixture daemon reports is anchored at. */
export const HOME = '/mock/.hestia';

const instances: InstanceInfo[] = [
  {
    id: 'fabric-playground',
    name: 'Fabric Playground',
    flavor: 'fabric',
    gameVersion: '1.21.1',
    loaderVersion: '0.16.5',
    javaMajor: 21,
    createdUnix: ago(86_400 * 12),
    lastPlayedUnix: ago(3_600),
    playtimeSeconds: 7_240,
    accepts: accepts('fabric', false),
    sessions: [],
  },
  {
    id: 'vanilla-survival',
    name: 'Vanilla Survival',
    flavor: 'vanilla',
    gameVersion: '1.21.4',
    javaMajor: 21,
    createdUnix: ago(86_400 * 40),
    lastPlayedUnix: ago(86_400 * 3),
    playtimeSeconds: 154_800,
    accepts: accepts('vanilla', false),
    sessions: [],
  },
  {
    id: 'kitchen-sink',
    name: 'Kitchen Sink',
    flavor: 'neoforge',
    gameVersion: '1.21.1',
    loaderVersion: '21.1.95',
    javaMajor: 21,
    createdUnix: ago(86_400 * 5),
    playtimeSeconds: 0,
    accepts: accepts('neoforge', false),
    sessions: [],
  },
];

const JAVA_PLAYER_LISTS: PlayerList[] = ['whitelist', 'ops', 'banned'];

const servers: ServerInfo[] = [
  {
    id: 'smp',
    name: 'SMP',
    flavor: 'vanilla',
    edition: 'java',
    gameVersion: '1.21.4',
    javaMajor: 21,
    createdUnix: ago(86_400 * 30),
    ready: true,
    gamePort: 25_565,
    ports: [{ port: 25_565, protocol: 'tcp' }],
    playerLists: JAVA_PLAYER_LISTS,
    console: true,
    accepts: accepts('vanilla', true),
  },
  {
    id: 'creative',
    name: 'Creative',
    flavor: 'paper',
    edition: 'java',
    gameVersion: '1.21.1',
    loaderVersion: '196',
    javaMajor: 21,
    createdUnix: ago(86_400 * 8),
    ready: true,
    gamePort: 25_566,
    ports: [{ port: 25_566, protocol: 'tcp' }],
    playerLists: JAVA_PLAYER_LISTS,
    console: true,
    accepts: accepts('paper', true),
  },
  {
    id: 'bedrock-realm',
    name: 'Bedrock Realm',
    flavor: 'bedrock',
    edition: 'bedrock',
    gameVersion: '1.21.50',
    createdUnix: ago(86_400 * 3),
    ready: true,
    gamePort: 19_132,
    ports: [
      { port: 19_132, protocol: 'tcp' },
      { port: 19_134, protocol: 'udp' },
    ],
    playerLists: ['whitelist', 'ops'],
    console: true,
    accepts: accepts('bedrock', true),
  },
];

const settings = new Map<string, ConfigEntry[]>([
  [
    'fabric-playground',
    [
      { key: 'memory', value: '6G' },
      { key: 'jvm-args', value: '-XX:+UseG1GC' },
    ],
  ],
  [
    'smp',
    [
      { key: 'memory', value: '4G' },
      { key: 'jvm-args', value: '' },
      { key: 'motd', value: 'A Mock Server' },
      { key: 'max-players', value: '20' },
      { key: 'difficulty', value: 'normal' },
      { key: 'gamemode', value: 'survival' },
      { key: 'hardcore', value: 'false' },
      { key: 'pvp', value: 'true' },
      { key: 'online-mode', value: 'true' },
      { key: 'allow-flight', value: 'false' },
      { key: 'white-list', value: 'false' },
      { key: 'view-distance', value: '10' },
      { key: 'simulation-distance', value: '10' },
      { key: 'spawn-protection', value: '16' },
      { key: 'level-name', value: 'world' },
      { key: 'level-seed', value: '' },
      { key: 'server-port', value: '25565' },
      { key: 'enable-rcon', value: 'true' },
      { key: 'rcon.port', value: '25575' },
      { key: 'rcon.password', value: 'mock-rcon-password' },
    ],
  ],
]);

settings.set('bedrock-realm', [
  { key: 'server-name', value: 'A Mock Bedrock Server' },
  { key: 'max-players', value: '10' },
  { key: 'difficulty', value: 'normal' },
  { key: 'gamemode', value: 'survival' },
  { key: 'online-mode', value: 'true' },
  { key: 'level-name', value: 'Bedrock level' },
  { key: 'server-port', value: '19132' },
]);

const packs = new Map<string, InstalledModpack>();

export const listInstances = (): InstanceInfo[] => instances;
export const listServers = (): ServerInfo[] => servers;

const matches = (entry: { id: string; name: string }, ref: string): boolean =>
  entry.id === ref || entry.name === ref || slug(entry.name) === slug(ref);

export function findInstance(ref: string): InstanceInfo {
  const found = instances.find((entry) => matches(entry, ref));
  if (!found) fail('not_found', `no such instance: ${ref}`);
  return found;
}

export function findServer(ref: string): ServerInfo {
  const found = servers.find((entry) => matches(entry, ref));
  if (!found) fail('not_found', `no such server: ${ref}`);
  return found;
}

/** An id nothing else holds, derived from the display name like the daemon's. */
function mintId(name: string, taken: { id: string }[]): string {
  const base = slug(name);
  if (!taken.some((entry) => entry.id === base)) return base;
  let n = 2;
  while (taken.some((entry) => entry.id === `${base}-${n}`)) n += 1;
  return `${base}-${n}`;
}

export function addInstance(params: {
  name: string;
  flavor: string;
  version: string;
  loaderVersion?: string;
}): InstanceInfo {
  const name = params.name || 'New Instance';
  const instance: InstanceInfo = {
    id: mintId(name, instances),
    name,
    flavor: params.flavor || 'vanilla',
    gameVersion: params.version || '1.21.4',
    loaderVersion: params.loaderVersion,
    javaMajor: javaFor(params.version),
    createdUnix: now(),
    playtimeSeconds: 0,
    accepts: accepts(params.flavor || 'vanilla', false),
    sessions: [],
  };
  instances.unshift(instance);
  return instance;
}

export function addServer(params: {
  name: string;
  flavor: string;
  version: string;
  loaderVersion?: string;
  port?: number;
}): ServerInfo {
  const name = params.name || 'New Server';
  const bedrock = params.flavor === 'bedrock';
  const gamePort = params.port ?? (bedrock ? 19_132 : 25_565) + servers.length;
  const server: ServerInfo = {
    id: mintId(name, servers),
    name,
    flavor: params.flavor || 'vanilla',
    edition: bedrock ? 'bedrock' : 'java',
    gameVersion: params.version || '1.21.4',
    loaderVersion: params.loaderVersion,
    javaMajor: bedrock ? undefined : javaFor(params.version),
    createdUnix: now(),
    ready: true,
    gamePort: gamePort,
    ports: bedrock
      ? [
          { port: gamePort, protocol: 'tcp' },
          { port: gamePort + 2, protocol: 'udp' },
        ]
      : [{ port: gamePort, protocol: 'tcp' }],
    playerLists: bedrock ? ['whitelist', 'ops'] : JAVA_PLAYER_LISTS,
    console: true,
    accepts: accepts(params.flavor || 'vanilla', true),
  };
  servers.unshift(server);
  return server;
}

export function rename<T extends { id: string; name: string }>(
  entry: T,
  name: string,
): T {
  entry.name = name;
  return entry;
}

export function removeEntry(id: string, from: { id: string }[]): void {
  const at = from.findIndex((entry) => entry.id === id);
  if (at >= 0) from.splice(at, 1);
  settings.delete(id);
  packs.delete(id);
}

/** Where an entry's files live, as the details views report them. */
export function directories(
  id: string,
  kind: 'instances' | 'servers',
): { entryDir: string; dataDir: string } {
  return {
    entryDir: `${HOME}/${kind}/${id}`,
    dataDir: `${HOME}/${kind}/${id}/data`,
  };
}

export const entrySettings = (id: string): ConfigEntry[] =>
  settings.get(id) ?? [];

export function setEntrySetting(id: string, key: string, value: string): void {
  const entries = settings.get(id) ?? [];
  const at = entries.findIndex((entry) => entry.key === key);
  if (at >= 0) entries[at] = { key, value };
  else entries.push({ key, value });
  settings.set(id, entries);
}

export const packOf = (id: string): InstalledModpack | undefined =>
  packs.get(id);

export const setPack = (id: string, pack: InstalledModpack): void => {
  packs.set(id, pack);
};

export const clearPack = (id: string): void => {
  packs.delete(id);
};

const bedrockWorld = (
  folder: string,
  name: string,
  gameMode: WorldInfo['gameMode'],
  playedDaysAgo: number,
): WorldInfo => ({
  folder,
  name,
  read: true,
  version: '1.21.50',
  gameMode,
  difficulty: 'normal',
  hardcore: false,
  cheats: false,
  lastPlayedUnix: ago(86_400 * playedDaysAgo),
  sizeBytes: 38 * 1024 * 1024,
  icon: '',
});

const serverWorlds = new Map<string, WorldInfo[]>([
  [
    'bedrock-realm',
    [
      bedrockWorld('Bedrock level', 'Bedrock level', 'survival', 0),
      bedrockWorld('creative-city', 'Creative City', 'creative', 6),
    ],
  ],
]);

export const worldsOf = (id: string): WorldInfo[] => serverWorlds.get(id) ?? [];

export function importWorld(id: string, path: string, use: boolean): WorldInfo {
  const file = path.split(/[/\\]/).pop() ?? 'world.mcworld';
  const name = file.replace(/\.mcworld$/i, '');
  const world = bedrockWorld(name, name, 'survival', 0);
  const rest = worldsOf(id).filter((w) => w.folder !== name);
  serverWorlds.set(id, use ? [world, ...rest] : [...rest, world]);
  if (use) setEntrySetting(id, 'level-name', name);
  return world;
}
