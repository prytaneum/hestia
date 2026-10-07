/** Only the webview's drag-drop event yields a real path; a DOM drop hands back a sandboxed `File`. */
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
