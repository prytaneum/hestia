/**
 * `update.*` — the fixture reports an unmanaged install, which is what a
 * browser session is: nothing here can replace a running binary.
 */
import type { UpdateCheckResult } from '@/api/types';

import type { ChannelHandlers } from '../support';

export const channels: ChannelHandlers = {
  'update.check': (): UpdateCheckResult => ({
    current: '0.0.1-mock',
    install: 'unmanaged',
    channel: 'stable',
    available: undefined,
  }),
};
