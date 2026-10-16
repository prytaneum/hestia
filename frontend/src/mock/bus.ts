/**
 * The push half of the bridge. The desktop shell forwards every daemon event
 * on one Tauri event (`hestia:event`) and connection transitions on another;
 * the fixture daemon emits through the same two names, so `api/core/events`
 * multiplexes them without knowing it is talking to a mock.
 *
 * `emit` round-trips through `plugin:event|emit`, which `mockIPC` handles
 * itself — the router never sees it.
 */
import { emit } from '@tauri-apps/api/event';

import { CONNECTION_CHANNEL, EVENT_CHANNEL } from '@/api/core/events';

/** Publish one daemon event: what a `Topic` implementor sends over the wire. */
export function publish(topic: string, payload: Record<string, unknown>): void {
  void emit(EVENT_CHANNEL, { topic, payload });
}

/** Report a connection transition, as the shell's watcher does. */
export function connection(state: 'connected' | 'disconnected'): void {
  void emit(CONNECTION_CHANNEL, state);
}
