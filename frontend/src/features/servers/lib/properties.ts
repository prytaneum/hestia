import type { ConfigEntry } from '@/api';

/** Settings `server.config.list` leads with that are not `server.properties`. */
const RECORD_KEYS = new Set([
  'memory',
  'jvm-args',
  'backup-interval',
  'backup-retention',
]);

/** Keys the daemon owns and rejects a write to. */
const MANAGED_KEYS = new Set([
  'server-port',
  'enable-rcon',
  'rcon.port',
  'rcon.password',
]);

const SECRET_KEYS = new Set(['rcon.password']);

const CHOICES: Record<string, string[]> = {
  gamemode: ['survival', 'creative', 'adventure', 'spectator'],
  difficulty: ['peaceful', 'easy', 'normal', 'hard'],
};

/** What hardcore pins, mirroring the daemon's invariant. */
const HARDCORE_LOCKS: Record<string, string> = {
  difficulty: 'hard',
  gamemode: 'survival',
};

export type PropertyControl =
  | { type: 'toggle' }
  | { type: 'number' }
  | { type: 'choice'; options: string[] }
  | { type: 'text' };

export interface Property {
  key: string;
  value: string;
  control: PropertyControl;
  managed: boolean;
  secret: boolean;
}

/**
 * The file carries no types, so a control is read off the stored value. A
 * choice applies only while the value is one of its names: pre-1.13 servers
 * store gamemode and difficulty as numeric ids.
 */
function controlFor(key: string, value: string): PropertyControl {
  const options = CHOICES[key];
  if (options?.includes(value)) return { type: 'choice', options };
  if (value === 'true' || value === 'false') return { type: 'toggle' };
  if (/^-?\d+$/.test(value)) return { type: 'number' };
  return { type: 'text' };
}

export function serverProperties(
  config: ConfigEntry[] | undefined,
): Property[] {
  return (config ?? [])
    .filter((entry) => !RECORD_KEYS.has(entry.key))
    .map(({ key, value }) => ({
      key,
      value,
      control: controlFor(key, value),
      managed: MANAGED_KEYS.has(key),
      secret: SECRET_KEYS.has(key),
    }))
    .sort((a, b) => a.key.localeCompare(b.key));
}

/** The value hardcore forces on `key` given the drafted values, if any. */
export function hardcoreLock(
  key: string,
  values: Record<string, string>,
): string | undefined {
  return values.hardcore === 'true' ? HARDCORE_LOCKS[key] : undefined;
}

/**
 * The edits to send, ordered so the daemon's hardcore check passes at every
 * step: the keys hardcore pins are written before hardcore turns on, and after
 * it turns off.
 */
export function propertyEdits(
  properties: Property[],
  draft: Record<string, string>,
): ConfigEntry[] {
  const edits = properties
    .filter(({ key, value }) => key in draft && draft[key] !== value)
    .map(({ key }) => ({ key, value: draft[key] }));
  const enabling = draft.hardcore === 'true';
  const rank = (key: string) => {
    if (key === 'hardcore') return 1;
    if (key in HARDCORE_LOCKS) return enabling ? 0 : 2;
    return 0;
  };
  return edits.sort((a, b) => rank(a.key) - rank(b.key));
}
