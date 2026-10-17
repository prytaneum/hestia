/** The `config.*` channels. `home` and `autostart` are the reserved keys. */
import { call, tryCall } from './core/ipc';

export async function get(key: string): Promise<unknown | null> {
  const result = await tryCall('config.get', { key });
  return result?.value ?? null;
}

export async function set(key: string, value: unknown): Promise<void> {
  await call('config.set', { key, value });
}

export async function list(): Promise<Record<string, unknown>> {
  const result = await call('config.list');
  return result.entries;
}
