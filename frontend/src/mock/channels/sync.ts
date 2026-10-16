/**
 * `sync.*` — the catalogue of shared settings, each unit off until it is
 * enabled from a named instance.
 */
import type {
  InstanceSyncStatus,
  SyncConfig,
  SyncOption,
  SyncUnit,
  UnitState,
} from '@/api/types';

import * as entries from '../state/entries';
import { type Handlers, str, strings } from '../support';

const UNITS: SyncUnit[] = [
  'options',
  'servers',
  'commands',
  'hotbars',
  'screenshots',
  'resource_packs',
  'data_packs',
];

const enabled = new Map<SyncUnit, string>([['options', 'Cozy']]);
let unsynced: string[] = [];
let options: SyncOption[] = [
  { key: 'guiScale', value: '2', synced: true },
  { key: 'fov', value: '80', synced: true },
  { key: 'renderDistance', value: '12', synced: true },
];

/** Units an instance keeps to itself, by id. */
const excluded = new Map<string, Set<SyncUnit>>();
const instanceKeys = new Map<string, string[]>();

const config = (): SyncConfig => ({
  sharedDir: `${entries.HOME}/shared`,
  units: UNITS.map((unit) => ({
    unit,
    enabled: enabled.has(unit),
    seededFrom: enabled.get(unit) ?? '',
  })),
  unsynced,
});

const state = (unit: SyncUnit, id: string, index: number): UnitState => {
  if (!enabled.has(unit)) return 'off';
  if (excluded.get(id)?.has(unit)) return 'overridden';
  return index === 0 ? 'synced' : 'pending';
};

const status = (): InstanceSyncStatus[] =>
  entries.listInstances().map((instance, index) => ({
    id: instance.id,
    name: instance.name,
    units: UNITS.map((unit) => ({
      unit,
      state: state(unit, instance.id, index),
    })),
    unsynced: instanceKeys.get(instance.id) ?? [],
  }));

const one = (id: string): InstanceSyncStatus =>
  status().find((instance) => instance.id === id) as InstanceSyncStatus;

const unitOf = (p: Record<string, unknown>): SyncUnit =>
  (p.unit as SyncUnit) ?? 'options';

export const channels: Handlers = {
  'sync.get': config,

  'sync.sources': () => ({
    sources: entries.listInstances().map((instance, index) => ({
      id: instance.id,
      name: instance.name,
      present: index < 2,
      modifiedUnix: Math.floor(Date.now() / 1000) - index * 86_400,
    })),
  }),

  'sync.enable': (p) => {
    const source = str(p, 'source');
    const from = source || entries.listInstances()[0]?.name || '';
    enabled.set(unitOf(p), from);
    return config();
  },

  'sync.disable': (p) => {
    enabled.delete(unitOf(p));
    return config();
  },

  'sync.options.keys': (p) => {
    unsynced = strings(p, 'unsynced');
    options = options.map((option) => ({
      ...option,
      synced: !unsynced.includes(option.key),
    }));
    return config();
  },

  'sync.options.get': () => ({ options }),

  'sync.options.set': (p) => {
    const key = str(p, 'key');
    const value = str(p, 'value');
    const existing = options.find((option) => option.key === key);
    if (existing) existing.value = value;
    else options.push({ key, value, synced: !unsynced.includes(key) });
    return { options };
  },

  'sync.status': () => ({ instances: status() }),

  'instance.sync.unit': (p) => {
    const instance = entries.findInstance(str(p, 'instance'));
    const unit = unitOf(p);
    const mine = excluded.get(instance.id) ?? new Set<SyncUnit>();
    if (p.shared === false) mine.add(unit);
    else mine.delete(unit);
    excluded.set(instance.id, mine);
    return one(instance.id);
  },

  'instance.sync.keys': (p) => {
    const instance = entries.findInstance(str(p, 'instance'));
    instanceKeys.set(instance.id, strings(p, 'unsynced'));
    return one(instance.id);
  },
};
