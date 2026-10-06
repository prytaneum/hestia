import type { Icon } from '@phosphor-icons/react';
import { type ReactNode, useState } from 'react';

import { cn } from '@/lib/utils';

const sizes = {
  xs: { box: 'size-6', glyph: 'size-3.5' },
  sm: { box: 'size-7', glyph: 'size-4' },
  md: { box: 'size-8', glyph: 'size-4.5' },
  lg: { box: 'size-9', glyph: 'size-5' },
  xl: { box: 'size-12', glyph: 'size-6' },
  '2xl': { box: 'size-16', glyph: 'size-8' },
  full: { box: 'aspect-square w-full', glyph: 'size-1/3' },
} as const;

/** Below this an icon is pixel art, and smoothing it only smears it. */
const PIXEL_ART_WIDTH = 32;

/**
 * The square image-or-glyph tile a list row, card or hero leads with.
 *
 * The box is 1:1 at every size and the image is *contained*, never cropped —
 * a picked icon of any aspect sits whole on the tile rather than losing its
 * edges to a centre crop.
 */
export function Thumbnail({
  src,
  fallback,
  icon: Glyph,
  size = 'sm',
  className,
  children,
}: {
  /** A URL or data URI; the glyph stands in when it is absent or fails. */
  src?: string;
  /** Tried when `src` is absent or fails, before the glyph. */
  fallback?: string;
  icon: Icon;
  size?: keyof typeof sizes;
  className?: string;
  /** Overlays positioned against the tile — a hover action, a menu. */
  children?: ReactNode;
}) {
  // Keyed by source, not a flag: a tile whose image changes underneath it must
  // get a fresh attempt, and must not carry the old one's rendering.
  const [broken, setBroken] = useState<string[]>([]);
  const [pixelArt, setPixelArt] = useState<string | null>(null);
  const { box, glyph } = sizes[size];
  const shown = [src, fallback].find(
    (candidate) => candidate && !broken.includes(candidate),
  );

  return (
    <span
      className={cn(
        'relative grid shrink-0 place-items-center overflow-hidden bg-muted text-muted-foreground ring-1 ring-border',
        box,
        className,
      )}
    >
      {!shown ? (
        <Glyph className={glyph} />
      ) : (
        <img
          src={shown}
          alt=""
          onError={() => setBroken((failed) => [...failed, shown])}
          onLoad={(e) =>
            setPixelArt(
              e.currentTarget.naturalWidth > 0 &&
                e.currentTarget.naturalWidth < PIXEL_ART_WIDTH
                ? shown
                : null,
            )
          }
          className={cn(
            'size-full object-contain',
            pixelArt === shown && '[image-rendering:pixelated]',
          )}
        />
      )}
      {children}
    </span>
  );
}

/**
 * The first of `sources` that carries anything, as an inline PNG. Locally-read
 * icons travel as bare base64 rather than as a path, so the webview's asset
 * protocol never has to reach into the data home.
 */
export function pngSource(...sources: (string | undefined)[]) {
  const source = sources.find((candidate) => !!candidate);
  return source ? `data:image/png;base64,${source}` : undefined;
}
