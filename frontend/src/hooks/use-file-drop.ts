/**
 * Dropping files from the desktop onto the window.
 *
 * The webview's drag-drop event is the only way to learn a real filesystem
 * path from a drop — an HTML `DataTransfer` hands back a sandboxed `File`, and
 * the daemon needs a path it can open itself.
 *
 * Only files carrying an accepted extension arm the drop: what a file really
 * is gets decided by the daemon reading it, but lighting a target up for a
 * dragged screenshot would be a lie about what will happen.
 */
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useEffect, useRef, useState } from 'react';
import { logger } from '@/lib/log';

const log = logger('drop');

const hasExtension = (path: string, extensions: readonly string[]) => {
  const lower = path.toLowerCase();
  return extensions.some((extension) => lower.endsWith(`.${extension}`));
};

export function useFileDrop(
  extensions: readonly string[],
  onDrop: (paths: string[]) => void,
): boolean {
  const [over, setOver] = useState(false);
  // Held in refs so a caller passing an inline closure or array does not tear
  // the listener down and set it up again on every render.
  const handler = useRef(onDrop);
  handler.current = onDrop;
  const accepted = useRef(extensions);
  accepted.current = extensions;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === 'leave') {
          setOver(false);
          return;
        }
        const paths = 'paths' in payload ? payload.paths : [];
        const matching = paths.filter((path) =>
          hasExtension(path, accepted.current),
        );
        if (payload.type === 'drop') {
          setOver(false);
          if (matching.length > 0) handler.current(matching);
          return;
        }
        // `over` repeats without paths; only `enter` knows what is dragged.
        if (payload.type === 'enter') setOver(matching.length > 0);
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      })
      .catch((error) => log.warn('cannot listen for dropped files', error));

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return over;
}
