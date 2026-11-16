/** `host.*` — a fixed 16 GB machine. */
import type { HostInfoResult } from '@/api/types';

import type { ChannelHandlers } from '../support';

export const channels: ChannelHandlers = {
  'host.info': (): HostInfoResult => ({
    totalMemoryBytes: 16 * 1024 ** 3,
  }),
};
