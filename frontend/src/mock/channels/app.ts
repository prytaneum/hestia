/** `app.*` and `daemon.*` — the fixture daemon talking about itself. */
import type { AppInfoResult, DaemonStatusResult } from '@/api/types';

import { connection } from '../bus';
import { HOME } from '../state/entries';
import type { ChannelHandlers } from '../support';

const PID = 4_242;
const started = Date.now();

export const status = (): DaemonStatusResult => ({
  pid: PID,
  version: '0.0.1',
  uptimeSeconds: Math.floor((Date.now() - started) / 1000),
  home: HOME,
  log: `${HOME}/logs/hestiad.log`,
  safeMode: false,
  quarantined: [],
});

export const channels: ChannelHandlers = {
  'app.info': (): AppInfoResult => ({
    name: 'Hestia',
    version: '0.0.1',
    id: 'org.prytaneum.hestia',
    vendor: 'prytaneum',
    channel: 'dev',
  }),

  'daemon.status': status,

  // Nothing here to stop, but the shell reports the transition — so the
  // offline banner behaves, and `start_daemon` is what brings it back.
  'daemon.stop': () => {
    connection('disconnected');
    return { stopping: true };
  },
};
