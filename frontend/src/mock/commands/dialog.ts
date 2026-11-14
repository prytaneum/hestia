/**
 * `tauri-plugin-dialog` — the native pickers. A browser cannot open one, and
 * an `<input type=file>` would hand back a sandboxed `File` rather than the
 * absolute path the daemon needs, so the fixture answers with a path shaped
 * like the filter asked for. That keeps the flows behind a picker — importing
 * a local file, choosing an icon, exporting an instance — reachable in dev.
 */
import type { Handlers } from '../support';

const PICKED = '/mock/downloads';

interface Filter {
  name: string;
  extensions: string[];
}

interface Options {
  multiple?: boolean;
  directory?: boolean;
  defaultPath?: string;
  filters?: Filter[];
}

const optionsOf = (payload: Record<string, unknown>): Options =>
  (payload.options ?? {}) as Options;

/** A file the first filter would accept; `hestia` stands in for a directory. */
function sample(options: Options): string {
  if (options.directory) return `${PICKED}/instances`;
  const extension = options.filters?.[0]?.extensions?.[0] ?? 'bin';
  const names: Record<string, string> = {
    jar: 'sodium-fabric-1.21.1.jar',
    png: 'icon.png',
    hestia: 'fabric-playground.hestia',
  };
  return `${PICKED}/${names[extension] ?? `picked.${extension}`}`;
}

export const commands: Handlers = {
  'plugin:dialog|open': (p) => {
    const options = optionsOf(p);
    const path = sample(options);
    return options.multiple ? [path] : path;
  },

  'plugin:dialog|save': (p) => {
    const options = optionsOf(p);
    const extension = options.filters?.[0]?.extensions?.[0] ?? 'bin';
    const name = (options.defaultPath ?? 'export').replace(/\.[^./]+$/, '');
    return `${PICKED}/${name}.${extension}`;
  },
};
