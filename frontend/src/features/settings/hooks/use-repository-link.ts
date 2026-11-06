/**
 * Repository links the OS hands the app.
 *
 * `hestia://add-repo?url=` reaches the shell, which validates it as https and
 * forwards the index URL — at launch as a pending value, afterwards as an
 * event. The Repositories section answers it with its add confirmation;
 * nothing is added until the user confirms there.
 */
import { listen } from '@tauri-apps/api/event';
import { useEffect } from 'react';
import { system } from '@/api';
import { logger } from '@/lib/log';

const log = logger('repository-link');

const REPOSITORY_LINK = 'repository-link';

let pending: string | null = null;
const listeners = new Set<(url: string) => void>();

let reveal: () => void = () => {};

function deliver(url: string): void {
  reveal();
  if (listeners.size === 0) {
    pending = url;
    return;
  }
  for (const listener of listeners) listener(url);
}

let started = false;

/**
 * Start receiving repository links. Called once, at app bootstrap; `show`
 * brings up the page whose Repositories section answers them.
 */
export function watchRepositoryLinks(show: () => void): void {
  if (started) return;
  started = true;
  reveal = show;
  system
    .pendingRepository()
    .then((url) => {
      if (url) deliver(url);
    })
    .catch((error) => log.warn('cannot read the repository link', error));
  listen<string>(REPOSITORY_LINK, (event) => deliver(event.payload)).catch(
    (error) => log.warn('cannot listen for repository links', error),
  );
}

/**
 * Run `onOpen` for each repository link the OS hands the app while this
 * component is mounted — including one that arrived before it was.
 */
export function useRepositoryLink(onOpen: (url: string) => void): void {
  useEffect(() => {
    listeners.add(onOpen);
    if (pending) {
      const url = pending;
      pending = null;
      onOpen(url);
    }
    return () => {
      listeners.delete(onOpen);
    };
  }, [onOpen]);
}
