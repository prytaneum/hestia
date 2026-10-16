/**
 * The reference data a provider would answer with: which flavors exist, what
 * each one takes, the game versions they publish, and their loader builds.
 * Read-only — nothing in the fixture daemon rewrites a catalogue.
 */
import type { ContentKind, Flavor, GameVersion, KeyRule } from '@/api/types';

const CLIENT_KINDS: ContentKind[] = [
  'mod',
  'resource_pack',
  'shader',
  'data_pack',
];
const SERVER_KINDS: ContentKind[] = ['mod', 'data_pack'];
const PLUGIN_KINDS: ContentKind[] = ['plugin', 'data_pack'];

const vanilla: Flavor = {
  id: 'vanilla',
  name: 'Vanilla',
  summary: "Mojang's game, unmodified. Takes datapacks and nothing else.",
  accepts: ['data_pack'],
  requires: [],
};

const fabric = (accepts: ContentKind[]): Flavor => ({
  id: 'fabric',
  name: 'Fabric',
  summary: 'Lightweight mod loader, quick to support each new game version.',
  accepts,
  requires: [],
});

const neoforge = (accepts: ContentKind[]): Flavor => ({
  id: 'neoforge',
  name: 'NeoForge',
  summary: 'The Forge successor. Its game jar is built locally at create.',
  accepts,
  requires: [],
});

export const serverRules: KeyRule[] = [
  { key: 'max-players', kind: { type: 'integer', min: 1, max: 2147483647 } },
  { key: 'server-port', kind: { type: 'integer', min: 1, max: 65535 } },
];

export const instanceFlavors: Flavor[] = [
  vanilla,
  fabric(CLIENT_KINDS),
  neoforge(CLIENT_KINDS),
];

export const serverFlavors: Flavor[] = [
  vanilla,
  fabric(SERVER_KINDS),
  neoforge(SERVER_KINDS),
  {
    id: 'paper',
    name: 'Paper',
    summary: 'Optimised Bukkit server with a plugin ecosystem.',
    accepts: PLUGIN_KINDS,
    requires: [],
  },
  {
    id: 'folia',
    name: 'Folia',
    summary: 'Regionised multithreading, for servers with spread-out players.',
    accepts: PLUGIN_KINDS,
    requires: [],
  },
  {
    id: 'spigot',
    name: 'Spigot',
    summary:
      'Compiled on your machine with BuildTools — a create takes a few minutes.',
    accepts: PLUGIN_KINDS,
    requires: [{ name: 'git', url: 'https://git-scm.com/downloads' }],
  },
];

export const versions: GameVersion[] = [
  { id: '1.21.4', kind: 'release', stable: true },
  { id: '1.21.3', kind: 'release', stable: true },
  { id: '1.21.1', kind: 'release', stable: true },
  { id: '1.21', kind: 'release', stable: true },
  { id: '25w03a', kind: 'snapshot', stable: false },
  { id: '1.20.6', kind: 'release', stable: true },
  { id: '1.20.1', kind: 'release', stable: true },
];

const LOADERS: Record<string, string[]> = {
  fabric: ['0.16.9', '0.16.5', '0.15.11'],
  neoforge: ['21.1.95', '21.1.80'],
  paper: ['196', '195'],
  folia: ['28'],
};

/** Loader builds for a flavor, newest first; empty for the ones without any. */
export function loaders(flavor: string): string[] {
  return LOADERS[flavor] ?? [];
}

/** The game versions a flavor publishes — every flavor here tracks them all. */
export function versionsFor(_flavor: string): GameVersion[] {
  return versions;
}

/** What a flavor accepts, for an entry record's `accepts`. */
export function accepts(flavor: string, server: boolean): ContentKind[] {
  const table = server ? serverFlavors : instanceFlavors;
  return table.find((f) => f.id === flavor)?.accepts ?? ['data_pack'];
}

/** The Java a game version wants, as the provider resolves it. */
export function javaFor(version: string): number {
  if (version.startsWith('1.20') || version.startsWith('1.21')) return 21;
  return 17;
}
