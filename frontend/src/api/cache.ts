/** The `cache.*` channels. */
import { call } from './core/ipc';
import type { CacheInfoResult, CacheUsage } from './types/cache';

export function info(): Promise<CacheInfoResult> {
  return call('cache.info');
}

/** Returns what was reclaimed. */
export function clear(): Promise<CacheUsage> {
  return call('cache.clear');
}
