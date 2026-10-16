/**
 * The one seam every daemon call crosses: the shell's generic `ipc_call`
 * command forwards `{ channel, payload }` over the local socket and answers
 * with the response payload or a `{ code, message }` rejection.
 *
 * The wire is camelCase — `crates/proto` carries `rename_all = "camelCase"`, so
 * the daemon speaks the same shape the type mirrors describe and no key
 * conversion happens here. (A dynamic-key payload like `config.*`'s opaque
 * value therefore also passes through untouched.)
 */
import { invoke } from '@tauri-apps/api/core';
import { logger } from '@/lib/log';
import type { ErrorInfo } from '../types/error';

const log = logger('ipc');

/** Error codes raised by the daemon (`ipc::errors`). */
export const NOT_FOUND = 'not_found';
export const BAD_REQUEST = 'bad_request';
export const HANDLER_ERROR = 'handler_error';
export const UNKNOWN_CHANNEL = 'unknown_channel';
export const UNAUTHORIZED = 'unauthorized';
/** The daemon could not reach upstream — distinct from the socket being down. */
export const OFFLINE = 'offline';
/** Error codes raised by the shell's bridge for transport failures. */
export const TIMEOUT = 'timeout';
export const CONNECTION_LOST = 'connection_lost';
export const TRANSPORT = 'transport';

export class HestiaError extends Error {
  readonly code: string;
  /** The structured daemon error (`proto::error::ErrorInfo`) to localize from. */
  readonly info: ErrorInfo | null;

  constructor(code: string, message: string, info: ErrorInfo | null = null) {
    super(message);
    this.name = 'HestiaError';
    this.code = code;
    this.info = info;
  }
}

export function isNotFound(error: unknown): boolean {
  return error instanceof HestiaError && error.code === NOT_FOUND;
}

export interface CallOptions {
  timeoutMs?: number;
}

function toHestiaError(raw: unknown): HestiaError {
  if (raw && typeof raw === 'object' && 'code' in raw && 'message' in raw) {
    const { code, message, info } = raw as {
      code: string;
      message: string;
      info?: ErrorInfo | null;
    };
    return new HestiaError(code, message, info ?? null);
  }
  return new HestiaError(TRANSPORT, String(raw));
}

export async function call<T>(
  channel: string,
  params: unknown = {},
  options: CallOptions = {},
): Promise<T> {
  const started = performance.now();
  try {
    const result = await invoke<unknown>('ipc_call', {
      channel,
      payload: params ?? {},
      timeoutMs: options.timeoutMs,
    });
    log.debug({ channel, ms: elapsed(started) }, 'call ok');
    return result as T;
  } catch (raw) {
    const error = toHestiaError(raw);
    log[daemonUnreachable(error) ? 'debug' : 'warn'](
      { channel, code: error.code, ms: elapsed(started) },
      `call failed: ${error.message}`,
    );
    throw error;
  }
}

function daemonUnreachable(error: HestiaError): boolean {
  return error.code === CONNECTION_LOST || error.code === TRANSPORT;
}

function elapsed(started: number): number {
  return Math.round(performance.now() - started);
}

/**
 * Invoke a bespoke shell command (not the generic `ipc_call` bridge). Reserved
 * for the few flows the frontend cannot drive over the socket alone — sign-in,
 * which opens a native webview and reads its redirect. Rejections share
 * `ipc_call`'s `{ code, message }` shape.
 */
export async function invokeCommand<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (raw) {
    const error = toHestiaError(raw);
    log.warn({ command, code: error.code }, `command failed: ${error.message}`);
    throw error;
  }
}

/** Like `call`, but a `not_found` answer becomes `null` instead of throwing. */
export async function tryCall<T>(
  channel: string,
  params: unknown = {},
  options: CallOptions = {},
): Promise<T | null> {
  try {
    return await call<T>(channel, params, options);
  } catch (error) {
    if (isNotFound(error)) return null;
    throw error;
  }
}
