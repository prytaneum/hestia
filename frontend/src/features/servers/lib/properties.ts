import type { ServerProperty } from '@/api';

export type PropertyControl = 'toggle' | 'number' | 'text';

/**
 * The file carries no types and its key set moves with the game version, so a
 * control is read off the shape of the server's own default — never off the
 * key's name. A key with no default falls back to the value it holds.
 */
export function propertyControl(property: ServerProperty): PropertyControl {
  const sample = property.default ?? property.value;
  if (sample === 'true' || sample === 'false') return 'toggle';
  if (/^-?\d+$/.test(sample)) return 'number';
  return 'text';
}

/** The properties whose key contains `query`, case-insensitively. */
export function matchProperties(
  properties: ServerProperty[],
  query: string,
): ServerProperty[] {
  const needle = query.trim().toLowerCase();
  return properties.filter(({ key }) => key.toLowerCase().includes(needle));
}
