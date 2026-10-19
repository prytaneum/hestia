import type { ComponentProps } from 'react';

import { system } from '@/api';
import { notify } from '@/lib/notify';

/**
 * A link that opens in the system browser rather than navigating the webview.
 * Every external link goes through it: the shell opens nothing on its own.
 */
export function ExternalLink({ href, onClick, ...rest }: ComponentProps<'a'>) {
  return (
    <a
      href={href}
      {...rest}
      onClick={(event) => {
        event.preventDefault();
        onClick?.(event);
        if (!href) return;
        system
          .openUrl(href)
          .catch((error: Error) =>
            notify.failure(error, { id: `open-url:${href}` }),
          );
      }}
    />
  );
}
