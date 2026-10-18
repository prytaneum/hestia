import { useState } from 'react';

/**
 * Runs `reset` during render on the closed-to-open transition, so a reopened
 * dialog never renders its previous draft first.
 */
export function useOnOpen(open: boolean, reset: () => void) {
  const [wasOpen, setWasOpen] = useState(open);

  if (wasOpen !== open) {
    setWasOpen(open);
    if (open) reset();
  }
}
