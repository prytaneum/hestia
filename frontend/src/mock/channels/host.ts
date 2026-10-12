/** `host.*` — a fixed 16 GB machine. */
import type { HostInfoResult } from '@/api/types';

import type { Handlers } from '../support';

export const channels: Handlers = {
  'host.info': (): HostInfoResult => ({
    totalMemoryBytes: 16 * 1024 ** 3,
  }),
};
