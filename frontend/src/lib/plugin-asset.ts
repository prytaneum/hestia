import { convertFileSrc } from '@tauri-apps/api/core';

const SCHEME = 'hestia-plugin';

/**
 * The URL the webview loads a plugin's file from (`assets/…` or `themes/…`).
 * The daemon serves it, so plugin content never shares the app's origin.
 *
 * `convertFileSrc` picks the scheme's form per platform
 * (`hestia-plugin://localhost/…`, or `http://hestia-plugin.localhost/…` on
 * WebView2) but encodes its whole argument as one segment. Only the root is
 * taken from it: the slashes stay real so a stylesheet's relative `url()`
 * resolves beside it.
 */
export function pluginAsset(plugin: string, path: string): string {
  const root = convertFileSrc('', SCHEME);
  const segments = [plugin, ...path.split('/')].map(encodeURIComponent);
  return `${root}${segments.join('/')}`;
}
