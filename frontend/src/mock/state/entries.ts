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
  ServerInfo,
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

const servers: ServerInfo[] = [
  {
    id: 'smp',
    name: 'SMP',
    flavor: 'vanilla',
    gameVersion: '1.21.4',
    javaMajor: 21,
    createdUnix: ago(86_400 * 30),
    ready: true,
    gamePort: 25_565,
    console: true,
    accepts: accepts('vanilla', true),
  },
  {
    id: 'creative',
    name: 'Creative',
    flavor: 'paper',
    gameVersion: '1.21.1',
    loaderVersion: '196',
    javaMajor: 21,
    createdUnix: ago(86_400 * 8),
    ready: true,
    gamePort: 25_566,
    console: true,
    accepts: accepts('paper', true),
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
      { key: 'backup-interval', value: '6h' },
      { key: 'backup-retention', value: '10' },
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
  const server: ServerInfo = {
    id: mintId(name, servers),
    name,
    flavor: params.flavor || 'vanilla',
    gameVersion: params.version || '1.21.4',
    loaderVersion: params.loaderVersion,
    javaMajor: javaFor(params.version),
    createdUnix: now(),
    ready: true,
    gamePort: params.port ?? 25_565 + servers.length,
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
