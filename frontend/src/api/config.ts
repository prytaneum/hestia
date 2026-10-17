/** The `config.*` channels. `home` and `autostart` are the reserved keys. */
import { call, tryCall } from './core/ipc';
import { relaunch } from './daemon';

export async function get(key: string): Promise<unknown | null> {
  const result = await tryCall('config.get', { key });
  return result?.value ?? null;
}

export async function set(key: string, value: unknown): Promise<void> {
  await call('config.set', { key, value });
}

/** The daemon stops itself once `home` is set; start it again on the new one. */
export async function moveHome(dir: string): Promise<void> {
  await set('home', dir);
  await relaunch();
}

export async function list(): Promise<Record<string, unknown>> {
  const result = await call('config.list');
  return result.entries;
}
