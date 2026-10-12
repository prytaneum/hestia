/** The `host.*` channels. */
import { call } from './core/ipc';
import type { HostInfoResult } from './types/host';

/** Facts about the machine the daemon runs on. */
export function info(): Promise<HostInfoResult> {
  return call('host.info');
}
