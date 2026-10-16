/**
 * `icon_*` — custom entry icons, copied into the data home by the shell and
 * served over the asset protocol. A browser has no such copy to make, so a set
 * records the picked path and `convertFileSrc` (mocked in ../internals) is
 * what turns it into something the webview would load.
 */
import { bool, type Handlers, now, str } from '../support';

interface IconEntry {
  path: string;
  mtime: number;
}

interface GeneratedIcon {
  config: unknown;
  generated: boolean;
}

const icons = new Map<string, IconEntry>();
const generated = new Map<string, GeneratedIcon>();

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

  icon_generate: (p) => {
    const entry = { config: p?.config, generated: bool(p, 'generate') };
    generated.set(str(p, 'entryId'), entry);
    return entry.config;
  },

  icon_config: (p) => generated.get(str(p, 'entryId'))?.config ?? null,
};
