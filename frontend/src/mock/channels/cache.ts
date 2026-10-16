/** `cache.*` — the content-addressed download cache. */
import type { CacheEntry, CacheInfoResult, CacheUsage } from '@/api/types';

import { HOME } from '../state/entries';
import type { Handlers } from '../support';

let entries: CacheEntry[] = Array.from({ length: 6 }, (_, index) => ({
  size: 4_200_000 + index * 812_000,
  algorithm: 'sha1',
  hex: index.toString(16).repeat(40).slice(0, 40),
}));

const usage = (): CacheUsage => ({
  entries: entries.length,
  bytes: entries.reduce((total, entry) => total + entry.size, 0),
});

export const channels: Handlers = {
  'cache.info': (): CacheInfoResult => ({ path: `${HOME}/cache`, ...usage() }),
  'cache.clear': () => {
    const reclaimed = usage();
    entries = [];
    return reclaimed;
  },
};
