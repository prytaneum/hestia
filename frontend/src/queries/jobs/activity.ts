import { useSyncExternalStore } from 'react';

interface ActivityState {
  open: boolean;
  /** The row the panel opens onto, when something pointed at one. */
  focus: string | null;
}

let state: ActivityState = { open: false, focus: null };
const listeners = new Set<() => void>();

function set(next: ActivityState): void {
  state = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Open the Activity panel, onto one job's row when given. */
export function openActivity(jobId?: string): void {
  set({ open: true, focus: jobId ?? null });
}

export function setActivityOpen(open: boolean): void {
  if (open === state.open) return;
  set({ open, focus: null });
}

export function useActivity(): ActivityState {
  return useSyncExternalStore(subscribe, () => state);
}
