import { useEffect, useState } from 'react';

import { findAnchor, type TourAnchor } from './anchor';
import type { Box } from './placement';

const RESOLVE_TIMEOUT_MS = 1500;

interface AnchorTracking {
  rect: Box | null;
  missing: boolean;
}

function boxOf(element: Element): Box {
  const { top, left, width, height } = element.getBoundingClientRect();
  return { top, left, width, height };
}

function same(a: Box | null, b: Box | null): boolean {
  if (!a || !b) return a === b;
  return (
    a.top === b.top &&
    a.left === b.left &&
    a.width === b.width &&
    a.height === b.height
  );
}

export function useAnchorRect(id: TourAnchor | undefined): AnchorTracking {
  const [rect, setRect] = useState<Box | null>(null);
  const [missing, setMissing] = useState(false);

  useEffect(() => {
    setRect(null);
    setMissing(false);
    if (!id) return;

    let frame = 0;
    let current: Box | null = null;
    let scrolled = false;
    const deadline = performance.now() + RESOLVE_TIMEOUT_MS;

    const tick = () => {
      const element = findAnchor(id);
      if (element) {
        if (!scrolled) {
          scrolled = true;
          element.scrollIntoView({ block: 'nearest', inline: 'nearest' });
        }
        const next = boxOf(element);
        if (!same(current, next)) {
          current = next;
          setRect(next);
        }
      } else if (performance.now() > deadline) {
        setMissing(true);
        return;
      }
      frame = requestAnimationFrame(tick);
    };

    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [id]);

  return { rect, missing };
}
