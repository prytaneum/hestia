import {
  ArrowsClockwiseIcon,
  CheckCircleIcon,
  CloudSlashIcon,
  GearSixIcon,
  ProhibitIcon,
  WifiSlashIcon,
} from '@phosphor-icons/react';
import { Link } from '@tanstack/react-router';
import type { ReactNode } from 'react';

import { Page } from '@/components/page';
import { Button, buttonVariants } from '@/components/ui/button';
import { agoLabel } from '@/lib/format';
import { m } from '@/paraglide/messages.js';
import { invalidate } from '@/queries/client';
import { keys } from '@/queries/keys';
import { useNetwork } from '@/queries/net';

/**
 * The whole story about being offline, in one place: which state the launcher
 * is in, what still works, and what does not. The per-surface empty states say
 * only "not now"; this is where someone goes to find out why.
 *
 * It never blocks anything — reaching it is a navigation, not a redirect, since
 * an offline launcher is still a working one.
 */
export function OfflinePage() {
  const network = useNetwork();
  const pinned = network?.offlineMode === true;
  const checking = network?.state === 'unknown';

  // Re-reading the state alone would only echo what the daemon already told us.
  // Sweeping the upstream-backed queries makes real requests, and those are
  // what the reachability state is derived from.
  const retry = () => {
    invalidate(keys.net.status());
    invalidate(keys.content.all);
  };

  return (
    <Page title={m['app.network.label']()}>
      <div className="mx-auto flex h-full max-w-xl flex-col items-center justify-center gap-8 py-10 text-center">
        <div className="flex flex-col items-center gap-4">
          {pinned ? (
            <CloudSlashIcon
              weight="duotone"
              className="size-12 text-muted-foreground"
            />
          ) : (
            <WifiSlashIcon
              weight="duotone"
              className="size-12 text-muted-foreground"
            />
          )}
          <div className="space-y-2">
            <h2 className="text-xl font-semibold">
              {pinned
                ? m['app.network.page_pinned_title']()
                : m['app.network.page_title']()}
            </h2>
            <p className="text-sm text-muted-foreground">
              {pinned
                ? m['app.network.page_pinned_body']()
                : m['app.network.page_body']()}
            </p>
            {!pinned && network?.lastOnlineUnix ? (
              <p className="text-xs text-muted-foreground/80">
                {m['app.network.last_online']({
                  ago: agoLabel(network.lastOnlineUnix),
                })}
              </p>
            ) : null}
          </div>
        </div>

        {pinned ? (
          <Link
            to="/settings"
            data-icon="inline-start"
            className={buttonVariants()}
          >
            <GearSixIcon weight="bold" />
            {m['app.network.open_settings']()}
          </Link>
        ) : (
          <Button
            variant="outline"
            data-icon="inline-start"
            disabled={checking}
            onClick={retry}
          >
            <ArrowsClockwiseIcon weight="bold" />
            {checking
              ? m['app.network.checking_again']()
              : m['app.network.retry']()}
          </Button>
        )}

        <div className="grid w-full gap-3 text-left sm:grid-cols-2">
          <Capability
            icon={
              <CheckCircleIcon weight="duotone" className="size-4 text-ember" />
            }
            title={m['app.network.works_title']()}
          >
            {m['app.network.works_body']()}
          </Capability>
          <Capability
            icon={
              <ProhibitIcon
                weight="duotone"
                className="size-4 text-muted-foreground"
              />
            }
            title={m['app.network.blocked_title']()}
          >
            {m['app.network.blocked_body']()}
          </Capability>
        </div>
      </div>
    </Page>
  );
}

function Capability({
  icon,
  title,
  children,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-1.5 border border-border p-4">
      <h3 className="flex items-center gap-2 text-sm font-medium">
        {icon}
        {title}
      </h3>
      <p className="text-xs text-muted-foreground">{children}</p>
    </div>
  );
}
