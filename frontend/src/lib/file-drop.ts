/**
 * The one pipeline every dropped file goes through, behaving as
 * `react-dropzone` does: a dropzone is an element, and a drop goes to the
 * innermost dropzone under the pointer that accepts the file. So nothing has to
 * know about dialogs — an open one covers the page, and the page behind it is
 * simply not what the pointer is over.
 *
 * The library itself cannot be used here: a path only comes from the webview's
 * own drag-drop event (a DOM drop hands back a sandboxed `File`), and that
 * event is window-wide. It says where the pointer is, not what it is over, so
 * the hit test is done by hand.
 */
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { logger } from '@/lib/log';

const log = logger('drop');

export interface Dropzone {
  /** Accepted file extensions, without the dot. */
  accept: readonly string[];
  onDrop: (paths: string[]) => void;
  /** False takes only the first accepted file. */
  multiple: boolean;
  /** Shown on the window-wide overlay; a zone without one highlights itself. */
  label?: string;
}

/** A pointer position in CSS pixels, relative to the viewport. */
export interface Point {
  x: number;
  y: number;
}

type DropEvent =
  | { type: 'enter'; paths: string[]; position: Point }
  | { type: 'over'; position: Point }
  | { type: 'drop'; paths: string[]; position: Point }
  | { type: 'leave' };

const zones = new WeakMap<Element, Dropzone>();
const listeners = new Set<() => void>();
let active: Dropzone | null = null;
// `over` repeats without paths; only `enter` knows what is being dragged.
let dragged: string[] = [];

function setActive(next: Dropzone | null): void {
  if (active === next) return;
  active = next;
  for (const listener of listeners) listener();
}

/** Accepts every file, whatever its extension. */
export const ANY_FILE = '*';

function accepted(zone: Dropzone, paths: string[]): string[] {
  const matching = paths.filter((path) => {
    if (zone.accept.includes(ANY_FILE)) return true;
    const lower = path.toLowerCase();
    return zone.accept.some((extension) =>
      lower.endsWith(`.${extension.toLowerCase()}`),
    );
  });
  return zone.multiple ? matching : matching.slice(0, 1);
}

function hit(
  position: Point,
  paths: string[],
): { zone: Dropzone; paths: string[] } | null {
  let element = document.elementFromPoint(position.x, position.y);
  while (element) {
    const zone = zones.get(element);
    const matching = zone ? accepted(zone, paths) : [];
    if (zone && matching.length > 0) return { zone, paths: matching };
    element = element.parentElement;
  }
  return null;
}

/** Feed one drag-drop event through the pipeline. */
export function dispatchDrop(event: DropEvent): void {
  switch (event.type) {
    case 'enter':
      dragged = event.paths;
      setActive(hit(event.position, dragged)?.zone ?? null);
      return;
    case 'over':
      setActive(hit(event.position, dragged)?.zone ?? null);
      return;
    case 'leave':
      dragged = [];
      setActive(null);
      return;
    case 'drop': {
      const landed = hit(event.position, event.paths);
      dragged = [];
      setActive(null);
      if (landed) {
        log.debug({ files: landed.paths.length }, 'files dropped');
        landed.zone.onDrop(landed.paths);
      }
      return;
    }
  }
}

/** Make an element a dropzone until the returned cleanup is called. */
export function registerDropzone(element: Element, zone: Dropzone): () => void {
  zones.set(element, zone);
  return () => {
    zones.delete(element);
    if (active === zone) setActive(null);
  };
}

/** The dropzone the current drag would land on, or null when there is none. */
export function activeDropzone(): Dropzone | null {
  return active;
}

export function subscribeDropzone(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * The webview reports the pointer in physical pixels on Windows and in logical
 * ones on Linux, under the same type. `elementFromPoint` takes CSS pixels.
 */
function toCssPixels(position: Point): Point {
  const scale = navigator.userAgent.includes('Windows')
    ? window.devicePixelRatio
    : 1;
  return { x: position.x / scale, y: position.y / scale };
}

let started = false;

/** Start receiving dropped files. Called once, at app bootstrap. */
export function watchFileDrops(): void {
  if (started) return;
  started = true;
  getCurrentWebview()
    .onDragDropEvent(({ payload }) => {
      if (payload.type === 'leave') {
        dispatchDrop({ type: 'leave' });
        return;
      }
      const position = toCssPixels(payload.position);
      if (payload.type === 'over') {
        dispatchDrop({ type: 'over', position });
      } else {
        dispatchDrop({ type: payload.type, paths: payload.paths, position });
      }
    })
    .catch((error) => log.warn('cannot listen for dropped files', error));
}
