import { FileArrowUpIcon } from '@phosphor-icons/react';
import { useSyncExternalStore } from 'react';

import { activeDropzone, subscribeDropzone } from '@/lib/file-drop';

/**
 * The one window-wide "drop here" affordance. It names the dropzone the drag
 * would land on; a zone with no label highlights itself, so nothing is drawn
 * over it. It ignores the pointer, so it never hides the zone it describes
 * from the hit test.
 */
export function FileDropOverlay() {
  const zone = useSyncExternalStore(subscribeDropzone, activeDropzone);
  if (!zone?.label) return null;

  return (
    <div className="pointer-events-none fixed inset-0 z-50 grid place-items-center bg-background/80 backdrop-blur-xs">
      <div className="flex flex-col items-center gap-3 border-2 border-dashed border-primary px-10 py-8">
        <FileArrowUpIcon className="size-8 text-primary" />
        <p className="font-medium text-sm">{zone.label}</p>
      </div>
    </div>
  );
}
