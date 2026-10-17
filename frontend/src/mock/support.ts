/**
 * Shared vocabulary for the fixture daemon: the handler shape both registries
 * are built from, the clock every record is stamped by, and the payload
 * readers that keep handlers free of `String(p.x ?? '')` noise.
 */

import type { Channels } from '@/api';
import type { Empty } from '@/api/types/generated/Empty';

/** Answers one daemon channel or one shell command. */
export type Handler = (payload: Record<string, unknown>) => unknown;

/** A domain's contribution to the shell-command registry, keyed by command name. */
export type Handlers = Record<string, Handler>;

/**
 * A domain's contribution to the channel registry. Keyed and answered by the
 * wire contract, so a fixture that stops matching it fails typecheck.
 */
export type ChannelHandlers = {
  [K in keyof Channels]?: (
    payload: Record<string, unknown>,
  ) => Channels[K]['result'];
};

/** Unix seconds, the stamp every proto record carries. */
export const now = (): number => Math.floor(Date.now() / 1000);

/** Unix seconds, `seconds` in the past — how the seed data is dated. */
export const ago = (seconds: number): number => now() - seconds;

/** What a channel whose result carries nothing answers with. */
export const ok = (): Empty => ({});

export function str(
  payload: Record<string, unknown>,
  key: string,
  fallback = '',
): string {
  const value = payload[key];
  return typeof value === 'string' && value !== '' ? value : fallback;
}

export function num(
  payload: Record<string, unknown>,
  key: string,
  fallback = 0,
): number {
  const value = payload[key];
  return typeof value === 'number' ? value : fallback;
}

export function bool(
  payload: Record<string, unknown>,
  key: string,
  fallback = false,
): boolean {
  const value = payload[key];
  return typeof value === 'boolean' ? value : fallback;
}

export function strings(
  payload: Record<string, unknown>,
  key: string,
): string[] {
  const value = payload[key];
  return Array.isArray(value) ? value.map(String) : [];
}

/**
 * Reject the way the bridge does. The daemon answers a failure as
 * `{ code, message }`, which `api/core/ipc` turns into a `HestiaError` — so a
 * `not_found` from here reaches `tryCall` as the `null` it expects.
 */
export function fail(code: string, message: string): never {
  throw { code, message };
}

/** The id a display name is stored under, mirroring `proto::naming::slugify`. */
export function slug(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '') || 'entry'
  );
}
