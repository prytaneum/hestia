export const serverProcessId = (id: string): string => `server-${id}`;

export const instanceSessionPrefix = (id: string): string => `instance-${id}_`;

export const instanceSessionId = (id: string, seq: number): string =>
  `${instanceSessionPrefix(id)}${seq}`;

/** The entry id a supervisor process id names, or null for the other kind. */
export function entryOfProcess(
  processId: string,
  kind: 'server' | 'instance',
): string | null {
  const prefix = `${kind}-`;
  if (!processId.startsWith(prefix)) return null;
  return processId.slice(prefix.length).split('_')[0] || null;
}
