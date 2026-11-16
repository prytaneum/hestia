import { Link } from '@tanstack/react-router';

import { Notifications } from '@/components/app-shell/notifications';
import { StatusDot } from '@/components/ui/status-dot';
import { anchor } from '@/features/onboarding';
import { m } from '@/paraglide/messages.js';
import { useDaemon } from '@/queries/daemon';
import { useNetwork } from '@/queries/net';

export function StatusBar() {
  const daemon = useDaemon();
  return (
    <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-border bg-sidebar px-4 text-[11px] text-muted-foreground">
      <span
        className="inline-flex items-center gap-1.5"
        {...anchor('daemon-status')}
      >
        <StatusDot tone={daemon.connected ? 'on' : 'off'} />
        {daemon.connected
          ? m['app.daemon.connected']()
          : m['app.daemon.offline']()}
      </span>
      {daemon.connected && <NetworkIndicator />}
      {daemon.status && (
        <span className="font-mono">v{daemon.status.version}</span>
      )}

      <Notifications />
    </footer>
  );
}

/**
 * Reachability beside the daemon light, and deliberately separate from it: the
 * daemon can be perfectly healthy with no way out to the internet, and the two
 * failures need different things from the user.
 */
function NetworkIndicator() {
  const network = useNetwork();
  const state = network?.state ?? 'unknown';
  const label =
    state === 'online'
      ? m['app.network.online']()
      : state === 'unknown'
        ? m['app.network.checking']()
        : network?.offlineMode
          ? m['app.network.pinned']()
          : m['app.network.offline']();

  const title =
    state === 'offline'
      ? network?.offlineMode
        ? m['app.network.pinned_label']()
        : m['app.network.offline_label']()
      : m['app.network.label']();

  const dot = (
    <StatusDot
      tone={state === 'online' ? 'on' : state === 'unknown' ? 'off' : 'warn'}
    />
  );

  // Offline is the one state with somewhere to go: the page explains what still
  // works. Online has nothing to say, so it stays inert text.
  if (state !== 'offline') {
    return (
      <span className="inline-flex items-center gap-1.5" title={title}>
        {dot}
        {label}
      </span>
    );
  }

  return (
    <Link
      to="/offline"
      className="inline-flex items-center gap-1.5 hover:text-foreground"
      title={title}
    >
      {dot}
      {label}
    </Link>
  );
}
