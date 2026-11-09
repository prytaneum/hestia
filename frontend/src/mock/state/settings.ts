/**
 * The daemon settings store. Held as the nested tree `config.list` answers
 * with — kebab-case keys, since the config vocabulary is the one place the
 * wire is not camelCase — and addressed by dotted path, as `config.get`/`set`
 * do. `home` and `autostart` sit beside the schema: the daemon routes those
 * two itself rather than storing them.
 */
import { HOME } from './entries';

type Tree = Record<string, unknown>;

const settings: Tree = {
  home: HOME,
  autostart: false,
  developer: false,
  defaults: { memory: '4G', 'jvm-args': '' },
  content: { 'curseforge-key': '' },
  discord: { enabled: true },
  instance: { 'multi-session': false },
  modpack: {
    'default-excludes': true,
    'exclude-files': '',
    'force-include-files': '',
    'overrides-exclusions': '',
  },
};

export const all = (): Tree => settings;

/** The value at a dotted path, or null when nothing is stored there. */
export function get(key: string): unknown {
  const path = key.split('.');
  let node: unknown = settings;
  for (const segment of path) {
    if (typeof node !== 'object' || node === null) return null;
    node = (node as Tree)[segment];
  }
  return node ?? null;
}

export function set(key: string, value: unknown): void {
  const path = key.split('.');
  const leaf = path.pop();
  if (!leaf) return;
  let node: Tree = settings;
  for (const segment of path) {
    if (typeof node[segment] !== 'object' || node[segment] === null)
      node[segment] = {};
    node = node[segment] as Tree;
  }
  node[leaf] = value;
}

export const enabled = (key: string): boolean => get(key) === true;
