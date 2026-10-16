import { useCallback, useMemo, useRef, useSyncExternalStore } from 'react';
import {
  activeDropzone,
  type Dropzone,
  registerDropzone,
  subscribeDropzone,
} from '@/lib/file-drop';

interface DropzoneOptions {
  /** Accepted file extensions, without the dot. */
  accept: readonly string[];
  /** The accepted files' absolute paths. */
  onDrop: (paths: string[]) => void;
  /** False takes only the first accepted file. Defaults to true. */
  multiple?: boolean;
  disabled?: boolean;
  /** Shown on the window-wide overlay; a zone without one highlights itself. */
  label?: string;
}

/**
 * A file dropzone, shaped like `react-dropzone`'s hook: spread
 * `getRootProps()` on the element that takes the drop, and read `isDragActive`
 * to highlight it while a file it accepts is dragged over.
 */
export function useDropzone<T extends Element = HTMLElement>(
  options: DropzoneOptions,
) {
  const latest = useRef(options);
  latest.current = options;

  const zone = useMemo<Dropzone>(
    () => ({
      get accept() {
        return latest.current.accept;
      },
      get multiple() {
        return latest.current.multiple ?? true;
      },
      get label() {
        return latest.current.label;
      },
      onDrop: (paths) => latest.current.onDrop(paths),
    }),
    [],
  );

  const disabled = options.disabled ?? false;
  const ref = useCallback(
    (element: T | null) =>
      element && !disabled ? registerDropzone(element, zone) : undefined,
    [zone, disabled],
  );

  const isDragActive = useSyncExternalStore(
    subscribeDropzone,
    () => activeDropzone() === zone,
  );

  return { getRootProps: () => ({ ref }), isDragActive };
}
