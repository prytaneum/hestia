import { useCallback, useEffect, useRef, useState } from 'react';

export type SaveState = 'idle' | 'saving' | 'saved';

const SPINNER_DELAY_MS = 300;
const SPINNER_MIN_MS = 400;
const SAVED_MS = 2000;

/**
 * Walks a write through idle, saving, saved and back. A write that settles
 * inside the spinner delay goes straight to saved, and a spinner that did show
 * stays long enough to be read. A failure returns to idle: the MutationCache
 * has already reported it, so this hook stays silent.
 */
export function useSaveState() {
  const [state, setState] = useState<SaveState>('idle');
  const inflight = useRef(false);
  const savedTimer = useRef<number>(undefined);

  useEffect(() => () => window.clearTimeout(savedTimer.current), []);

  const run = useCallback(async (work: () => Promise<unknown>) => {
    if (inflight.current) return;
    inflight.current = true;
    window.clearTimeout(savedTimer.current);

    let spinnerShownAt: number | null = null;
    const spinner = window.setTimeout(() => {
      spinnerShownAt = Date.now();
      setState('saving');
    }, SPINNER_DELAY_MS);

    const ok = await work().then(
      () => true,
      () => false,
    );
    window.clearTimeout(spinner);

    if (spinnerShownAt !== null) {
      const remaining = SPINNER_MIN_MS - (Date.now() - spinnerShownAt);
      if (remaining > 0) {
        await new Promise((resolve) => window.setTimeout(resolve, remaining));
      }
    }
    inflight.current = false;

    if (!ok) {
      setState('idle');
      return;
    }
    setState('saved');
    savedTimer.current = window.setTimeout(() => setState('idle'), SAVED_MS);
  }, []);

  return { state, run };
}
