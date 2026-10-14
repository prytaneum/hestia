import type { ComponentProps } from 'react';
import { toast } from 'sonner';

import { errorMessage, system } from '@/api';

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
          .catch((error: Error) => toast.error(errorMessage(error)));
      }}
    />
  );
}
