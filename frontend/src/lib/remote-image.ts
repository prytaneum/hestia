import { convertFileSrc } from '@tauri-apps/api/core';

const SCHEME = 'hestia-image';

/**
 * The URL the webview loads a remote image from. The daemon fetches it, so the
 * webview never contacts the host; anything that is not an `https` URL (a data
 * URI, a local asset, a relative path) is returned unchanged.
 *
 * `convertFileSrc` picks the scheme's form per platform:
 * `hestia-image://localhost/…`, or `http://hestia-image.localhost/…` on
 * WebView2.
 */
export function remoteImage(src: string): string;
export function remoteImage(src: string | undefined): string | undefined;
export function remoteImage(src: string | undefined): string | undefined {
  if (!src?.toLowerCase().startsWith('https://')) return src;
  return `${convertFileSrc('fetch', SCHEME)}?u=${encodeURIComponent(src)}`;
}
