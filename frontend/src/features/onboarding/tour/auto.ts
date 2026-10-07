import { useEffect } from 'react';

import type { TourId } from './registry';
import { useTour } from './store';

/** Runs a page's tour the first time the page is ready and nothing else is up. */
export function useAutoTour(id: TourId, ready = true) {
  const { start, seen, idle } = useTour();
  const unseen = !seen(id);

  useEffect(() => {
    if (ready && idle && unseen) start(id);
  }, [ready, idle, unseen, id, start]);
}
