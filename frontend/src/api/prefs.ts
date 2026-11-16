/**
 * Desktop-local preferences: front-end UI state persisted straight into the
 * hestia data home by the shell, not through the daemon. Backed by the
 * `prefs_*` Tauri commands (see `crates/desktop/src/commands/prefs.rs`).
 */
import { invokeCommand } from './core/ipc';

export function list(): Promise<Record<string, unknown>> {
  return invokeCommand<Record<string, unknown>>('prefs_list');
}

export function set(key: string, value: unknown): Promise<void> {
  return invokeCommand<void>('prefs_set', { key, value });
}

export function remove(key: string): Promise<void> {
  return invokeCommand<void>('prefs_remove', { key });
}
