/**
 * Handing a path or a URL to the OS. A browser can do the URL half for real; a
 * filesystem path it can only report.
 */
import { type Handlers, str } from '../support';

export const commands: Handlers = {
  open_url: (p) => {
    const url = str(p, 'url');
    if (url) window.open(url, '_blank', 'noopener');
    return null;
  },

  open_path: (p) => {
    console.info('[mock] would open', str(p, 'path'));
    return null;
  },

  'plugin:opener|reveal_item_in_dir': (p) => {
    console.info('[mock] would reveal', (p.paths as string[])?.[0] ?? '');
    return null;
  },
};
