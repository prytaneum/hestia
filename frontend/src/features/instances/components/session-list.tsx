import { PowerIcon, TerminalWindowIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import type { ProcessInfo } from '@/api';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { StatusDot } from '@/components/ui/status-dot';
import { uptime } from '@/lib/format';
import { sessionStart } from '@/lib/sessions';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

/**
 * The instance's live sessions, one row each: a name, a lifetime, its own stop,
 * and the pick that points the overview's meters and the log view at it.
 */
export function SessionList({
  sessions,
  focused,
  onFocus,
  onLogs,
  onStop,
  stopping,
}: {
  sessions: ProcessInfo[];
  /** The session the overview's meters follow. */
  focused?: string;
  onFocus: (session: string) => void;
  onLogs: (session: string) => void;
  onStop: (session: string) => void;
  stopping?: boolean;
}) {
  const [confirm, setConfirm] = useState<ProcessInfo | null>(null);
  const now = Date.now() / 1000;

  return (
    <>
      <div className="divide-y divide-border border border-border">
        {sessions.map((session) => {
          const isFocused = session.id === focused;
          return (
            <div
              key={session.id}
              className={cn(
                'flex items-center gap-3 px-3 py-2.5',
                isFocused && 'bg-muted/40',
              )}
            >
              <StatusDot tone="on" />
              <button
                type="button"
                aria-pressed={isFocused}
                onClick={() => onFocus(session.id)}
                className="min-w-0 flex-1 text-left outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <div className="truncate text-sm">
                  {m['entry.session.name']({ time: sessionStart(session) })}
                </div>
                <div className="truncate font-mono text-[11px] text-muted-foreground">
                  {m['entry.session.pid']({ pid: session.pid })} ·{' '}
                  {uptime(now - session.startedUnix)}
                </div>
              </button>
              <Button
                size="sm"
                variant="ghost"
                data-icon="inline-start"
                className="shrink-0"
                onClick={() => onLogs(session.id)}
              >
                <TerminalWindowIcon />
                {m['app.label.logs']()}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                data-icon="inline-start"
                className="shrink-0"
                disabled={stopping}
                onClick={() => setConfirm(session)}
              >
                <PowerIcon weight="bold" />
                {m['app.action.stop']()}
              </Button>
            </div>
          );
        })}
      </div>

      <ConfirmDialog
        open={confirm != null}
        onOpenChange={(open) => {
          if (!open) setConfirm(null);
        }}
        title={m['entry.stop.session_title']({
          time: confirm ? sessionStart(confirm) : '',
        })}
        description={m['entry.stop.session_description']({
          count: sessions.length - 1,
        })}
        confirmLabel={m['app.action.stop']()}
        onConfirm={() => {
          if (confirm) onStop(confirm.id);
          setConfirm(null);
        }}
      />
    </>
  );
}
