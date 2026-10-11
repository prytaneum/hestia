/**
 * The one pipeline every dropped file goes through.
 *
 * The webview raises a single window-wide drag-drop event, so "which surface is
 * this drop for" cannot be answered by where the pointer is. It is answered
 * here instead: surfaces register as targets, and a drag is routed to the most
 * recently enabled one that accepts what is being dragged — a dialog opened
 * over a page takes the drop the page would otherwise get, and a file the
 * dialog does not take falls through to the page.
 *
 * Only this event yields a real path; a DOM drop hands back a sandboxed `File`.
 */
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { logger } from '@/lib/log';

const log = logger('drop');

export interface DropTarget {
  /** Accepted file extensions, without the dot. */
  extensions: readonly string[];
  onDrop: (paths: string[]) => void;
  /** Shown on the window-wide overlay; a target without one highlights itself. */
  label?: string;
}

export type DropEvent =
  | { type: 'enter'; paths: string[] }
  | { type: 'over' }
  | { type: 'drop'; paths: string[] }
  | { type: 'leave' };

const targets: DropTarget[] = [];
const listeners = new Set<() => void>();
let active: DropTarget | null = null;

function setActive(next: DropTarget | null): void {
  if (active === next) return;
  active = next;
  for (const listener of listeners) listener();
}

function accepted(target: DropTarget, paths: string[]): string[] {
  return paths.filter((path) => {
    const lower = path.toLowerCase();
    return target.extensions.some((extension) =>
      lower.endsWith(`.${extension.toLowerCase()}`),
    );
  });
}

function route(
  paths: string[],
): { target: DropTarget; paths: string[] } | null {
  for (let index = targets.length - 1; index >= 0; index -= 1) {
    const matching = accepted(targets[index], paths);
    if (matching.length > 0) return { target: targets[index], paths: matching };
  }
  return null;
}

/** Feed one drag-drop event through the pipeline. */
export function dispatchDrop(event: DropEvent): void {
  switch (event.type) {
    case 'enter':
      setActive(route(event.paths)?.target ?? null);
      return;
    case 'leave':
      setActive(null);
      return;
    case 'drop': {
      const hit = route(event.paths);
      setActive(null);
      if (hit) {
        log.debug({ files: hit.paths.length }, 'files dropped');
        hit.target.onDrop(hit.paths);
      }
      return;
    }
    // `over` repeats without paths; only `enter` knows what is dragged.
    case 'over':
      return;
  }
}

/**
 * Put a target on top of the stack. Registration order is the priority, so a
 * surface registers when it becomes able to take a drop — not when it mounts.
 */
export function registerDropTarget(target: DropTarget): () => void {
  targets.push(target);
  return () => {
    const index = targets.indexOf(target);
    if (index >= 0) targets.splice(index, 1);
    if (active === target) setActive(null);
  };
}

/** The target the current drag would land on, or null when nothing is dragged. */
export function activeDropTarget(): DropTarget | null {
  return active;
}

export function subscribeDropTarget(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

let started = false;

/** Start receiving dropped files. Called once, at app bootstrap. */
export function watchFileDrops(): void {
  if (started) return;
  started = true;
  getCurrentWebview()
    .onDragDropEvent(({ payload }) => {
      if (payload.type === 'enter' || payload.type === 'drop') {
        dispatchDrop({ type: payload.type, paths: payload.paths });
      } else {
        dispatchDrop({ type: payload.type });
      }
    })
    .catch((error) => log.warn('cannot listen for dropped files', error));
}
