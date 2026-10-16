/**
 * The `sync.*` channels — the catalogue of shared settings, what can seed it,
 * the shared game options, and each instance's standing.
 */
import { call } from './core/ipc';
import type {
  InstanceSyncStatus,
  SyncConfig,
  SyncOption,
  SyncSource,
  SyncUnit,
} from './types/sync';

export function get(): Promise<SyncConfig> {
  return call('sync.get');
}

/** Which instances could start a unit's shared copy. */
export async function sources(unit: SyncUnit): Promise<SyncSource[]> {
  const result = await call<{ sources: SyncSource[] }>('sync.sources', {
    unit,
  });
  return result.sources;
}

/** `source` may be empty only when at most one instance holds the file. */
export function enable(unit: SyncUnit, source = ''): Promise<SyncConfig> {
  return call('sync.enable', { unit, source });
}

export function disable(unit: SyncUnit): Promise<SyncConfig> {
  return call('sync.disable', { unit });
}

/** The `options.txt` keys no instance shares. */
export function setUnsynced(unsynced: string[]): Promise<SyncConfig> {
  return call('sync.options.keys', { unsynced });
}

export async function options(): Promise<SyncOption[]> {
  const result = await call<{ options: SyncOption[] }>('sync.options.get');
  return result.options;
}

export async function setOption(
  key: string,
  value: string,
): Promise<SyncOption[]> {
  const result = await call<{ options: SyncOption[] }>('sync.options.set', {
    key,
    value,
  });
  return result.options;
}

export async function status(): Promise<InstanceSyncStatus[]> {
  const result = await call<{ instances: InstanceSyncStatus[] }>('sync.status');
  return result.instances;
}

/** `shared` null returns the instance to following the catalogue. */
export function setInstanceUnit(
  instance: string,
  unit: SyncUnit,
  shared: boolean | null,
): Promise<InstanceSyncStatus> {
  return call('instance.sync.unit', { instance, unit, shared });
}

export function setInstanceUnsynced(
  instance: string,
  unsynced: string[],
): Promise<InstanceSyncStatus> {
  return call('instance.sync.keys', { instance, unsynced });
}
