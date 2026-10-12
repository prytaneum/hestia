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
 * The writes one change takes. Turning hardcore on first moves the keys it
 * pins, since the daemon refuses hardcore while they contradict it — the same
 * thing Minecraft's own create-world screen does.
 */
export function propertyWrites(
  properties: Property[],
  key: string,
  value: string,
): ConfigEntry[] {
  if (key !== 'hardcore' || value !== 'true') return [{ key, value }];
  const pins = properties
    .filter(
      (property) =>
        property.control.type === 'choice' &&
        property.key in HARDCORE_LOCKS &&
        property.value !== HARDCORE_LOCKS[property.key],
    )
    .map((property) => ({
      key: property.key,
      value: HARDCORE_LOCKS[property.key],
    }));
  return [...pins, { key, value }];
}
