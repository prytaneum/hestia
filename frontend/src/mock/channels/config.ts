/**
 * `config.*` — the settings store's dotted-path get/set and the whole tree.
 * An unknown key answers `null` rather than rejecting: the daemon refuses it,
 * but a mock that rejects would only turn a new setting into a dev-time crash.
 */
import * as settings from '../state/settings';
import { type ChannelHandlers, ok, str } from '../support';

export const channels: ChannelHandlers = {
  'config.get': (p) => ({ value: settings.get(str(p, 'key')) }),
  'config.list': () => ({ entries: settings.all() }),
  'config.set': (p) => {
    settings.set(str(p, 'key'), p.value);
    return ok();
  },
};
