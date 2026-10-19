import { useSyncExternalStore } from 'react';

export type NoticeKind = 'success' | 'info' | 'warning' | 'error';

export interface Notice {
  /** The toast id it was shown under; a repeat under it replaces the entry. */
  id: string;
  kind: NoticeKind;
  title: string;
  description?: string;
  action?: { label: string; onClick: () => void };
  at: number;
  read: boolean;
}

interface Centre {
  notices: Notice[];
  open: boolean;
}

const MAX_NOTICES = 100;

let centre: Centre = { notices: [], open: false };
const listeners = new Set<() => void>();

function set(next: Centre): void {
  centre = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Keep a notice for the session, newest first, whether or not it was toasted. */
export function record(notice: Omit<Notice, 'at' | 'read'>): void {
  const rest = centre.notices.filter((entry) => entry.id !== notice.id);
  set({
    ...centre,
    notices: [{ ...notice, at: Date.now(), read: false }, ...rest].slice(
      0,
      MAX_NOTICES,
    ),
  });
}

export function removeNotice(id: string): void {
  set({
    ...centre,
    notices: centre.notices.filter((entry) => entry.id !== id),
  });
}

export function clearNotices(): void {
  set({ ...centre, notices: [] });
}

/** Open or close the bell's panel; closing it marks everything read. */
export function setNotificationsOpen(open: boolean): void {
  if (open === centre.open) return;
  set({
    open,
    notices: open
      ? centre.notices
      : centre.notices.map((entry) =>
          entry.read ? entry : { ...entry, read: true },
        ),
  });
}

export function useNotificationCentre(): Centre {
  return useSyncExternalStore(subscribe, () => centre);
}
