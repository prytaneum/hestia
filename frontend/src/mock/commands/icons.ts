/**
 * `icons_allow` — a browser has no asset scope to widen, and `convertFileSrc`
 * (mocked in ../internals) is what turns an icon's path into a URL.
 */
import type { Handlers } from '../support';

export const commands: Handlers = {
  icons_allow: () => undefined,
};
