/** The `app.*` channels. */
import { call } from './core/ipc';
import type { AppInfoResult } from './types/app';

export function info(): Promise<AppInfoResult> {
  return call('app.info');
}
