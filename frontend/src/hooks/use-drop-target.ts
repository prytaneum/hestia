import { useEffect, useMemo, useRef, useSyncExternalStore } from 'react';
import {
  activeDropTarget,
  type DropTarget,
  registerDropTarget,
  subscribeDropTarget,
} from '@/lib/file-drop';

export interface DropTargetOptions extends DropTarget {
  /** Whether this surface can take a drop right now. Defaults to true. */
  enabled?: boolean;
}

/**
 * Register a surface with the file-drop pipeline. Returns whether a drag in
 * progress would land here, for a surface that highlights itself.
 */
export function useDropTarget({
  enabled = true,
  ...options
}: DropTargetOptions): boolean {
  const latest = useRef(options);
  latest.current = options;

  const target = useMemo<DropTarget>(
    () => ({
      get extensions() {
        return latest.current.extensions;
      },
      get label() {
        return latest.current.label;
      },
      onDrop: (paths) => latest.current.onDrop(paths),
    }),
    [],
  );

  useEffect(
    () => (enabled ? registerDropTarget(target) : undefined),
    [enabled, target],
  );

  return useSyncExternalStore(
    subscribeDropTarget,
    () => activeDropTarget() === target,
  );
}
