import { PushPinIcon } from '@phosphor-icons/react';

import { Button } from '@/components/ui/button';
import { m } from '@/paraglide/messages.js';
import { type PinnedEntry, usePinned } from '@/queries/pinned';

export function EntryPinButton(entry: PinnedEntry) {
  const { ready, isPinned, toggle } = usePinned();
  const pinned = isPinned(entry);
  const label = pinned ? m['app.label.unpin']() : m['app.label.pin']();

  return (
    <Button
      variant="outline"
      size="icon"
      aria-label={label}
      aria-pressed={pinned}
      title={label}
      disabled={!ready}
      onClick={() => toggle(entry)}
    >
      <PushPinIcon weight={pinned ? 'fill' : 'regular'} className="size-4" />
    </Button>
  );
}
