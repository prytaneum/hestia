/**
 * Pinned entries — the sidebar's pin list, persisted as a desktop-local pref.
 * One hook serves every pin surface (sidebar, entry cards) so they stay in
 * sync through the prefs cache.
 */
import { useMemo } from 'react';
import { usePrefs } from './prefs';

const PINNED_ENTRIES_KEY = 'sidebar.pinned-entries';

/** Stable empty fallback so the parse memo doesn't churn when nothing is pinned. */
const NO_PINS: unknown[] = [];

type PinnedKind = 'instance' | 'server';
export interface PinnedEntry {
  kind: PinnedKind;
  id: string;
}

export function pinKey(pin: PinnedEntry): string {
  return `${pin.kind}:${pin.id}`;
}

/** Validate the persisted blob, dropping malformed and duplicate entries. */
function parsePinnedEntries(value: unknown): PinnedEntry[] {
  if (!Array.isArray(value)) return [];

  const entries: PinnedEntry[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    if (typeof item !== 'object' || item === null) continue;
    const { kind, id } = item as Record<string, unknown>;
    if ((kind !== 'instance' && kind !== 'server') || typeof id !== 'string') {
      continue;
    }
    if (id === '') continue;

    const key = `${kind}:${id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    entries.push({ kind, id });
  }
  return entries;
}

export function usePinned() {
  const { get, set, ready } = usePrefs();

  const raw = get<unknown>(PINNED_ENTRIES_KEY, NO_PINS);
  const pins = useMemo(() => parsePinnedEntries(raw), [raw]);

  const save = (entries: PinnedEntry[]) =>
    set(
      PINNED_ENTRIES_KEY,
      entries.map(({ kind, id }) => ({ kind, id })),
    );

  const isPinned = (pin: PinnedEntry) =>
    pins.some((entry) => pinKey(entry) === pinKey(pin));

  const toggle = (pin: PinnedEntry) =>
    save(
      isPinned(pin)
        ? pins.filter((entry) => pinKey(entry) !== pinKey(pin))
        : [...pins, pin],
    );

  return { pins, ready, isPinned, toggle, save };
}
