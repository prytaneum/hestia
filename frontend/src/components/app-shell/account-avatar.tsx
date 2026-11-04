import { useEffect, useRef, useState } from 'react';

import { drawHead, HEAD, loadTexture } from '@/features/skins/lib';
import { remoteImage } from '@/lib/remote-image';
import { cn } from '@/lib/utils';

/** The face is 8×8; only a whole-number scale renders every pixel the same size. */
const FACE = 8;
const MAX_SCALE = 16;

/**
 * A Minecraft player-head avatar. When the account's equipped skin texture is
 * known locally it is blitted head-on, so a skin change reflects instantly —
 * the uuid-keyed mineatar face is cached by the browser (and for 12 hours by
 * the service) across an equip and lags behind. Without a texture it falls back
 * to mineatar by uuid, and to the name's initials when neither renders
 * (offline, unknown profile).
 */
export function AccountAvatar({
  uuid,
  name,
  size = 24,
  texture,
  bust,
  className,
}: {
  uuid: string;
  name: string;
  /** Rendered edge length in pixels, snapped to a multiple of the 8px face. */
  size?: number;
  /**
   * The equipped skin's texture (url or data url). Preferred over mineatar so a
   * change is reflected without waiting on the cached service face.
   */
  texture?: string;
  /**
   * Cache-bust token for the mineatar fallback. Its url is uuid-only, so a skin
   * change reuses it and the browser serves the stale face; pass the equipped
   * skin's key here to force a re-fetch when it changes.
   */
  bust?: string;
  className?: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [textureFailed, setTextureFailed] = useState(false);

  // Track the identity whose head failed, so switching account — or equipping a
  // new skin — re-tries the new head instead of staying on the fallback.
  const id = bust ? `${uuid}:${bust}` : uuid;
  const [failedId, setFailedId] = useState<string | null>(null);

  useEffect(() => {
    if (!texture) return;
    let live = true;
    setTextureFailed(false);
    loadTexture(texture)
      .then((img) => {
        if (live && canvasRef.current) drawHead(canvasRef.current, img);
      })
      .catch(() => {
        if (live) setTextureFailed(true);
      });
    return () => {
      live = false;
    };
  }, [texture]);

  const scale = Math.min(MAX_SCALE, Math.max(1, Math.round(size / FACE)));
  const edge = scale * FACE;
  const box = cn('shrink-0 overflow-hidden ring-1 ring-border', className);
  const style = { width: edge, height: edge };

  if (texture && !textureFailed) {
    return (
      <canvas
        ref={canvasRef}
        width={HEAD}
        height={HEAD}
        aria-hidden
        style={style}
        className={cn(box, 'bg-muted [image-rendering:pixelated]')}
      />
    );
  }

  if (!uuid || failedId === id) {
    return (
      <span
        style={style}
        className={cn(
          box,
          'grid place-items-center bg-muted font-semibold text-muted-foreground',
        )}
      >
        {name.slice(0, 2).toUpperCase()}
      </span>
    );
  }

  return (
    <img
      src={remoteImage(
        `https://api.mineatar.io/face/${uuid}?scale=${Math.min(MAX_SCALE, scale * 2)}${
          bust ? `&v=${encodeURIComponent(bust)}` : ''
        }`,
      )}
      alt=""
      width={edge}
      height={edge}
      style={style}
      onError={() => setFailedId(id)}
      className={cn(box, 'bg-muted [image-rendering:pixelated]')}
    />
  );
}
