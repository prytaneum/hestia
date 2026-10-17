/** `net.*` — reachability, driven by the `network.offline` mock setting. */
import type { NetworkStatus } from '@/api/types';

import type { ChannelHandlers } from '../support';

const startedAt = Math.floor(Date.now() / 1000);

/** Flip to exercise the offline surfaces under `vite dev`. */
const OFFLINE = false;

export const channels: ChannelHandlers = {
  'net.status': (): NetworkStatus => ({
    state: OFFLINE ? 'offline' : 'online',
    offlineMode: false,
    sinceUnix: startedAt,
    lastOnlineUnix: OFFLINE ? startedAt - 900 : Math.floor(Date.now() / 1000),
  }),
};
