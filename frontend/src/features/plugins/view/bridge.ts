/** What a plugin's sandboxed frame may ask of the host. Everything else is dropped. */
export type BridgeRequest =
  | {
      id: string;
      type: 'run';
      command: string;
      args: Record<string, unknown>;
    }
  | { id: string; type: 'action'; action: string; payload: unknown };

export type BridgeReply =
  | { id: string; ok: true; result: unknown }
  | { id: string; ok: false; error: string };

export const MAX_BRIDGE_BYTES = 64 * 1024;
const MAX_ID_LENGTH = 64;
const MAX_NAME_LENGTH = 128;

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function isName(value: unknown, max: number): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= max;
}

/**
 * Validates a frame's message and returns it as plain JSON, or `null` when it
 * is not exactly a request this host serves. The message is untrusted: it is
 * size-capped, must be a plain object, and is copied through JSON so no
 * prototype or non-JSON value reaches the daemon.
 */
export function parseBridgeMessage(data: unknown): BridgeRequest | null {
  if (!isPlainObject(data)) return null;
  let message: unknown;
  try {
    const text = JSON.stringify(data);
    if (text.length > MAX_BRIDGE_BYTES) return null;
    message = JSON.parse(text);
  } catch {
    return null;
  }
  if (!isPlainObject(message) || !isName(message.id, MAX_ID_LENGTH)) {
    return null;
  }
  const { id } = message;
  if (message.type === 'run') {
    const args = message.args ?? {};
    if (!isName(message.command, MAX_NAME_LENGTH) || !isPlainObject(args)) {
      return null;
    }
    return { id, type: 'run', command: message.command, args };
  }
  if (message.type === 'action') {
    if (!isName(message.action, MAX_NAME_LENGTH)) return null;
    return {
      id,
      type: 'action',
      action: message.action,
      payload: message.payload ?? null,
    };
  }
  return null;
}
