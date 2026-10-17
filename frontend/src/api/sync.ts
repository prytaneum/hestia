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
export function sources(unit: SyncUnit): Promise<SyncSource[]> {
  return call('sync.sources', { unit });
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
  const result = await call('sync.options.get');
  return result.options;
}

export async function setOption(
  key: string,
  value: string,
): Promise<SyncOption[]> {
  const result = await call('sync.options.set', { key, value });
  return result.options;
}

export function status(): Promise<InstanceSyncStatus[]> {
  return call('sync.status');
}

/** `shared` null returns the instance to following the catalogue. */
export function setInstanceUnit(
  instance: string,
  unit: SyncUnit,
  shared: boolean | null,
): Promise<InstanceSyncStatus> {
  return call('instance.sync.unit', {
    instance,
    unit,
    shared: shared ?? undefined,
  });
}

export function setInstanceUnsynced(
  instance: string,
  unsynced: string[],
): Promise<InstanceSyncStatus> {
  return call('instance.sync.keys', { instance, unsynced });
}
