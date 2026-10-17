/**
 * Desktop-only window behavior: hide the launcher while a game runs and restore
 * it when the last session exits, and answer the window's close the way the
 * user asked. Play accounting (last-played, playtime) is the daemon's now,
 * carried on the instance record and detail views.
 */
import { getCurrentWindow } from '@tauri-apps/api/window';
import { onDaemonEvent, type Topics } from '../api';
import { stop as stopDaemon } from '../api/daemon';
import { queryClient } from './client';
import { instanceQueries } from './instance';
import { prefsQueries } from './prefs';

/** The prefs key for "keep the launcher open while a game runs". */
const KEEP_OPEN_KEY = 'keepOpen';

/** The prefs key for what pressing close does. */
export const CLOSE_ACTION_KEY = 'closeAction';

export type CloseAction = 'quit' | 'stop-daemon' | 'tray';

export const CLOSE_ACTIONS: CloseAction[] = ['quit', 'tray', 'stop-daemon'];

const SESSION_ID = /^instance-(.+)_\d+$/;

const running = new Set<string>();
let hidden = false;
let started = false;

async function keepOpen(): Promise<boolean> {
  const all = await queryClient.ensureQueryData(prefsQueries.list());
  return (all[KEEP_OPEN_KEY] as boolean | undefined) ?? true;
}

async function hideWindow(): Promise<void> {
  if (hidden) return;
  hidden = true;
  await getCurrentWindow().hide();
}

async function restoreWindow(): Promise<void> {
  if (!hidden) return;
  hidden = false;
  const window = getCurrentWindow();
  await window.show();
  await window.setFocus();
}

async function onStarted({ id }: Topics['process.started']): Promise<void> {
  if (!SESSION_ID.test(id)) return;
  running.add(id);
  if (!(await keepOpen())) await hideWindow();
}

async function onExit({ id }: Topics['process.exit']): Promise<void> {
  if (!running.delete(id)) return;
  if (running.size === 0) await restoreWindow();
}

async function adoptRunning(): Promise<void> {
  const instances = await queryClient.ensureQueryData(instanceQueries.list());
  for (const instance of instances) {
    for (const session of instance.sessions ?? []) {
      if (session.state === 'running') running.add(session.id);
    }
  }
}

/**
 * What close does, read straight from the cache: `onCloseRequested` honors a
 * `preventDefault()` only while the handler still holds the event, so this
 * cannot be awaited.
 */
function closeAction(): CloseAction {
  const prefs = queryClient.getQueryData<Record<string, unknown>>(
    prefsQueries.list().queryKey,
  );
  const value = prefs?.[CLOSE_ACTION_KEY];
  return value === 'tray' || value === 'stop-daemon' ? value : 'quit';
}

/** Supervised workloads outlive the daemon, so closing never ends a game. */
async function stopThenClose(): Promise<void> {
  try {
    await stopDaemon(false);
  } catch {
    // Already down, or it never answered — the window closes regardless.
  }
  // `destroy` rather than `close`: the close we are answering must not recurse.
  await getCurrentWindow().destroy();
}

/** Install the window tracker once, at app bootstrap. */
export function startSessionTracking(): void {
  if (started) return;
  started = true;
  getCurrentWindow()
    .onCloseRequested((event) => {
      const action = closeAction();
      if (action === 'quit') return;
      event.preventDefault();
      if (action === 'tray') void getCurrentWindow().hide();
      else void stopThenClose();
    })
    .catch(() => {
      // Outside the Tauri shell there is no window to answer for.
    });
  onDaemonEvent((event) => {
    if (event.topic === 'process.started') void onStarted(event.payload);
    if (event.topic === 'process.exit') void onExit(event.payload);
  }).catch(() => {
    // Outside the Tauri shell there are no daemon events to hear.
  });
  adoptRunning().catch(() => {
    // No account yet (the instance surface is gated) or no daemon — fine.
  });
}
