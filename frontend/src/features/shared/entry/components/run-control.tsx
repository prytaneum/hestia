import { CaretDownIcon, PlayIcon, PowerIcon } from '@phosphor-icons/react';
import { Fragment, useState } from 'react';

import type { ProcessInfo } from '@/api';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { uptime } from '@/lib/format';
import { sessionSeq } from '@/lib/sessions';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useMultiSession } from '@/queries/config';

type Size = 'xs' | 'sm' | 'default';

const CARET_SIZE = {
  xs: 'icon-xs',
  sm: 'icon-sm',
  default: 'icon',
} as const;

/**
 * A stop names one `session` or, absent, every session of the instance; a
 * restart always names the one it relaunches.
 */
type Pending =
  | { action: 'stop'; session?: string }
  | { action: 'restart'; session: string };

/**
 * What a running instance offers: Stop, plus the menu a further launch, the
 * restarts and the per-session stops hang off. The menu appears only when
 * there is something in it — concurrent sessions are gated behind
 * `instance.multi-session`, and with that off and no `onRestart` a
 * single-session instance has nothing but its stop.
 *
 * Every handler stops propagation — the control renders inside the library's
 * card links, and the menu's portalled content still bubbles through the React
 * tree to them.
 */
export function EntryRunControl({
  name,
  sessions,
  size = 'default',
  iconOnly = false,
  busy = false,
  launching = false,
  onNewSession,
  onRestart,
  onStop,
}: {
  name: string;
  /** The live sessions, oldest first. */
  sessions: ProcessInfo[];
  size?: Size;
  /** Compact square buttons with no labels — the library card's hover action. */
  iconOnly?: boolean;
  busy?: boolean;
  launching?: boolean;
  onNewSession: () => void;
  /** Stop one session and launch it again; absent hides the option. */
  onRestart?: (session: string) => void;
  onStop: (session?: string) => void;
}) {
  const [confirm, setConfirm] = useState<Pending | null>(null);
  const multiSession = useMultiSession();
  const now = Date.now() / 1000;
  const target = confirm?.session;
  const seq = target ? sessionSeq(target) : 0;
  const restarting = confirm?.action === 'restart';
  const only = sessions.length === 1 ? sessions[0] : undefined;
  const menu = multiSession || sessions.length > 1 || onRestart != null;

  return (
    <>
      <div className="flex items-center">
        <Button
          variant="outline"
          size={iconOnly ? 'icon-sm' : size}
          aria-label={m['app.action.stop']()}
          title={m['app.action.stop']()}
          disabled={busy}
          className={cn(menu && 'border-r-0')}
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            setConfirm({ action: 'stop' });
          }}
        >
          <PowerIcon weight="bold" />
          {!iconOnly && m['app.action.stop']()}
        </Button>
        {menu && (
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  variant="outline"
                  size={iconOnly ? 'icon-sm' : CARET_SIZE[size]}
                  aria-label={m['entry.session.menu']()}
                  title={m['entry.session.menu']()}
                  disabled={busy}
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                  }}
                >
                  <CaretDownIcon weight="bold" />
                </Button>
              }
            />
            <DropdownMenuContent
              align="end"
              className="w-56"
              onClick={(event) => event.stopPropagation()}
            >
              {multiSession && (
                <>
                  <DropdownMenuItem disabled={launching} onClick={onNewSession}>
                    <PlayIcon weight="fill" />
                    {m['entry.session.new']()}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                </>
              )}
              <DropdownMenuItem onClick={() => setConfirm({ action: 'stop' })}>
                {sessions.length > 1
                  ? m['entry.session.stop_all']()
                  : m['app.action.stop']()}
              </DropdownMenuItem>
              {onRestart && only && (
                <DropdownMenuItem
                  disabled={launching}
                  onClick={() =>
                    setConfirm({ action: 'restart', session: only.id })
                  }
                >
                  {m['app.action.restart']()}
                </DropdownMenuItem>
              )}
              {sessions.length > 1 && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuLabel>
                    {m['app.label.sessions']()}
                  </DropdownMenuLabel>
                  {sessions.map((session) => (
                    <Fragment key={session.id}>
                      <DropdownMenuItem
                        onClick={() =>
                          setConfirm({ action: 'stop', session: session.id })
                        }
                      >
                        <span className="min-w-0 flex-1 truncate">
                          {m['entry.session.stop_one']({
                            seq: sessionSeq(session.id),
                          })}
                        </span>
                        <span className="font-mono text-[10px] text-muted-foreground">
                          {uptime(now - session.startedUnix)}
                        </span>
                      </DropdownMenuItem>
                      {onRestart && (
                        <DropdownMenuItem
                          disabled={launching}
                          onClick={() =>
                            setConfirm({
                              action: 'restart',
                              session: session.id,
                            })
                          }
                        >
                          {m['entry.session.restart_one']({
                            seq: sessionSeq(session.id),
                          })}
                        </DropdownMenuItem>
                      )}
                    </Fragment>
                  ))}
                </>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>

      <ConfirmDialog
        open={confirm != null}
        onOpenChange={(open) => {
          if (!open) setConfirm(null);
        }}
        title={
          restarting
            ? only
              ? m['entry.restart.title']({ name })
              : m['entry.restart.session_title']({ seq })
            : target
              ? m['entry.stop.session_title']({ seq })
              : m['entry.stop.title']({ name })
        }
        description={
          restarting
            ? m['entry.restart.instance_description']()
            : target
              ? m['entry.stop.session_description']({
                  count: sessions.length - 1,
                })
              : sessions.length > 1
                ? m['entry.stop.sessions_description']({
                    count: sessions.length,
                  })
                : m['entry.stop.instance_description']()
        }
        confirmLabel={
          restarting ? m['app.action.restart']() : m['app.action.stop']()
        }
        onConfirm={() => {
          if (confirm?.action === 'restart') onRestart?.(confirm.session);
          else onStop(target);
          setConfirm(null);
        }}
      />
    </>
  );
}
