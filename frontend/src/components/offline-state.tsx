import { CloudSlashIcon, WifiSlashIcon } from '@phosphor-icons/react';
import { Link } from '@tanstack/react-router';
import type { ReactNode } from 'react';

import { Empty } from '@/components/empty';
import { buttonVariants } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useNetwork } from '@/queries/net';

/**
 * What a page that needs the internet shows instead of its content.
 *
 * Unlike a lost daemon, a lost *connection* does not stop the app — instances
 * still launch and servers still run — so this replaces one surface rather than
 * covering the window. It distinguishes the two offline states because their
 * remedies differ: wait for a connection, or turn a setting off.
 */
export function OfflineState({
  action,
  className,
}: {
  action?: ReactNode;
  className?: string;
}) {
  const network = useNetwork();
  const pinned = network?.offlineMode === true;
  return (
    <Empty
      icon={pinned ? CloudSlashIcon : WifiSlashIcon}
      description={
        pinned ? m['app.network.pinned_body']() : m['app.network.body']()
      }
      action={
        action ?? (
          <Link
            to="/offline"
            className={buttonVariants({ variant: 'outline', size: 'sm' })}
          >
            {m['app.network.details']()}
          </Link>
        )
      }
      className={className}
    >
      {pinned ? m['app.network.pinned_label']() : m['app.network.title']()}
    </Empty>
  );
}

/**
 * An inline note that the actions on this surface need a connection. For a
 * surface whose *content* is local and only its downloads are not.
 */
export function OfflineNotice({ className }: { className?: string }) {
  return (
    <p
      className={cn(
        'flex items-center gap-1.5 text-xs text-muted-foreground',
        className,
      )}
    >
      <WifiSlashIcon className="size-3.5 shrink-0" />
      {m['app.network.needs_connection']()}
    </p>
  );
}

/**
 * An inline note that what is on screen came from the last time the launcher
 * was online — a cached catalogue rather than a live read.
 */
export function StaleNotice({ className }: { className?: string }) {
  return (
    <p
      className={cn(
        'flex items-center gap-1.5 text-xs text-muted-foreground',
        className,
      )}
    >
      <WifiSlashIcon className="size-3.5 shrink-0" />
      {m['app.network.stale']()}
    </p>
  );
}
