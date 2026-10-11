import { FileArrowUpIcon } from '@phosphor-icons/react';
import { useSyncExternalStore } from 'react';

import { activeDropTarget, subscribeDropTarget } from '@/lib/file-drop';

/**
 * The one window-wide "drop here" affordance. It names the target the drag
 * would land on; a target with no label is an inline drop zone that highlights
 * itself, so nothing is drawn over it.
 */
export function FileDropOverlay() {
  const target = useSyncExternalStore(subscribeDropTarget, activeDropTarget);
  if (!target?.label) return null;

  return (
    <div className="pointer-events-none fixed inset-0 z-50 grid place-items-center bg-background/80 backdrop-blur-xs">
      <div className="flex flex-col items-center gap-3 border-2 border-dashed border-primary px-10 py-8">
        <FileArrowUpIcon className="size-8 text-primary" />
        <p className="font-medium text-sm">{target.label}</p>
      </div>
    </div>
  );
}
