/**
 * `prefs_*` — the desktop's own preferences, stored by the shell rather than
 * the daemon (window behaviour, the library's pinned entries). Kept in
 * `sessionStorage` so a reload keeps what the page just set, and a new tab
 * starts from the defaults.
 */
import { type Handlers, str } from '../support';

const KEY = 'hestia.mock.prefs';

const DEFAULTS: Record<string, unknown> = {
  keepOpen: true,
  closeAction: 'quit',
};

function read(): Record<string, unknown> {
  try {
    return { ...DEFAULTS, ...JSON.parse(sessionStorage.getItem(KEY) ?? '{}') };
  } catch {
    return { ...DEFAULTS };
  }
}

function write(prefs: Record<string, unknown>): void {
  sessionStorage.setItem(KEY, JSON.stringify(prefs));
}

export const commands: Handlers = {
  safe_mode: () => false,

  prefs_list: () => read(),

  prefs_set: (p) => {
    const prefs = read();
    prefs[str(p, 'key')] = p.value;
    write(prefs);
    return null;
  },

  prefs_remove: (p) => {
    const prefs = read();
    delete prefs[str(p, 'key')];
    write(prefs);
    return null;
  },
};
