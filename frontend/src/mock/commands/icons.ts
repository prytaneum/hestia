/**
 * `icon_*` — custom entry icons, copied into the data home by the shell and
 * served over the asset protocol. A browser has no such copy to make, so a set
 * records the picked path and `convertFileSrc` (mocked in ../internals) is
 * what turns it into something the webview would load.
 */
import { type Handlers, now, str } from '../support';

interface IconEntry {
  path: string;
  mtime: number;
}

const icons = new Map<string, IconEntry>();
const configs = new Map<string, unknown>();

export const commands: Handlers = {
  icons_list: () => Object.fromEntries(icons),

  icon_set: (p) => {
    const entry = { path: str(p, 'sourcePath'), mtime: now() };
    icons.set(str(p, 'entryId'), entry);
    return entry;
  },

  icon_fetch: (p) => {
    const entry = { path: str(p, 'url'), mtime: now() };
    icons.set(str(p, 'entryId'), entry);
    return entry;
  },

  icon_remove: (p) => {
    icons.delete(str(p, 'entryId'));
    configs.delete(str(p, 'entryId'));
  },

  icon_set_bytes: (p) => {
    configs.set(str(p, 'entryId'), p?.config);
    return p?.config;
  },

  icon_config: (p) => configs.get(str(p, 'entryId')) ?? null,
};
