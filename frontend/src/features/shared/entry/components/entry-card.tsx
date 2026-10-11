import { PlayIcon, PowerIcon } from '@phosphor-icons/react';
import { createLink } from '@tanstack/react-router';
import { motion } from 'motion/react';

import type { ProcessInfo } from '@/api';
import { entryIcon } from '@/components/icons';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Spinner } from '@/components/ui/spinner';
import { StatusDot } from '@/components/ui/status-dot';
import { Thumbnail } from '@/components/ui/thumbnail';
import { layoutMorph, listItem } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import type { View } from './collection';
import { EntryContextMenu } from './entry-menu';
import { EntryRunControl } from './run-control';

export interface EntryCardModel {
  id: string;
  name: string;
  kind: 'instance' | 'server';
  flavor: string;
  version: string;
  running: boolean;
  ready: boolean;
  /** One-line footer: "Last played 2h ago" / ":25565 · 3 online". */
  subtitle: string;
  /** A custom icon shown in place of the kind glyph. */
  iconUrl?: string;
  /** An instance's live sessions, oldest first; a server has none. */
  sessions?: ProcessInfo[];
  /** Wired quick actions; absent leaves the button inert. */
  onStart?: () => void;
  /** `session` names one to stop; absent stops the entry outright. */
  onStop?: (session?: string) => void;
  /** Launch alongside the running sessions; absent hides the option. */
  onNewSession?: () => void;
  busy?: boolean;
  /** Split out of `busy` so a launch in flight still leaves stopping live. */
  launching?: boolean;
  stopping?: boolean;
}

const MotionLink = createLink(motion.a);

function statusOf(entry: EntryCardModel) {
  if (!entry.ready)
    return { tone: 'warn' as const, label: m['app.status.preparing']() };
  if (entry.running)
    return {
      tone: 'on' as const,
      label:
        entry.kind === 'server'
          ? m['app.status.online']()
          : m['app.status.running'](),
    };
  return null;
}

function detailTo(kind: 'instance' | 'server') {
  return kind === 'server' ? '/servers/$id' : '/instances/$id';
}

function StatusBadge({
  status,
  overlay,
}: {
  status: NonNullable<ReturnType<typeof statusOf>>;
  overlay?: boolean;
}) {
  return (
    <Badge
      variant="secondary"
      className={cn('gap-1.5', overlay && 'bg-background/80 backdrop-blur-xs')}
    >
      <StatusDot tone={status.tone} />
      {status.label}
    </Badge>
  );
}

function ActionButton({
  entry,
  size = 'sm',
  square = false,
}: {
  entry: EntryCardModel;
  size?: 'sm' | 'xs';
  /** Grid cards: a bare square icon, no label. */
  square?: boolean;
}) {
  if (entry.running && entry.onNewSession) {
    return (
      <EntryRunControl
        name={entry.name}
        sessions={entry.sessions ?? []}
        size={size}
        iconOnly={square}
        busy={entry.stopping}
        launching={entry.launching}
        onNewSession={entry.onNewSession}
        onStop={(session) => entry.onStop?.(session)}
      />
    );
  }
  if (entry.running) {
    return (
      <ConfirmDialog
        trigger={
          <Button
            variant="outline"
            size={square ? 'icon' : size}
            disabled={entry.busy}
            aria-label={m['app.action.stop']()}
            title={m['app.action.stop']()}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
            }}
          >
            <PowerIcon weight="bold" />
            {!square && m['app.action.stop']()}
          </Button>
        }
        title={m['entry.stop.title']({ name: entry.name })}
        description={
          entry.kind === 'server'
            ? m['entry.stop.server_description']()
            : m['entry.stop.instance_description']()
        }
        confirmLabel={m['app.action.stop']()}
        onConfirm={() => entry.onStop?.()}
      />
    );
  }
  return (
    <Button
      size={square ? 'icon-lg' : size}
      disabled={!entry.ready || entry.busy}
      aria-label={
        entry.kind === 'server'
          ? m['app.action.start']()
          : m['app.action.play']()
      }
      title={
        entry.kind === 'server'
          ? m['app.action.start']()
          : m['app.action.play']()
      }
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        entry.onStart?.();
      }}
      className="bg-ember text-ember-foreground hover:bg-ember/90"
    >
      {entry.busy ? <Spinner /> : <PlayIcon weight="fill" />}
      {!square &&
        (entry.kind === 'server'
          ? m['app.action.start']()
          : m['app.action.play']())}
    </Button>
  );
}

export function EntryTile({
  entry,
  view,
}: {
  entry: EntryCardModel;
  view: View;
}) {
  const status = statusOf(entry);
  const Icon = entryIcon(entry.kind);
  const grid = view === 'grid';

  return (
    <EntryContextMenu entry={entry}>
      <MotionLink
        layout
        variants={listItem}
        exit="exit"
        transition={layoutMorph}
        to={detailTo(entry.kind)}
        params={{ id: entry.id }}
        className={cn(
          'group relative flex overflow-hidden outline-none focus-visible:ring-1 focus-visible:ring-ring',
          grid
            ? 'flex-col border border-border bg-card transition-colors hover:border-ember/40'
            : 'items-center gap-3 px-3 py-2.5 transition-colors hover:bg-muted/40 focus-visible:ring-inset',
        )}
      >
        <motion.div
          layout
          transition={layoutMorph}
          className={cn('shrink-0', grid && 'w-full border-b border-border')}
        >
          <Thumbnail
            src={entry.iconUrl}
            icon={Icon}
            size={grid ? 'full' : 'lg'}
            className={cn(
              grid &&
                'bg-muted/40 ring-0 transition-[filter] group-hover:brightness-90',
            )}
          />
        </motion.div>

        {/* Text block: position-only. Glyphs must never be scaled, and this
            box's own size change isn't worth animating. */}
        <motion.div
          layout="position"
          transition={layoutMorph}
          className={cn('min-w-0', grid ? 'w-full p-3 pt-2.5' : 'flex-1')}
        >
          <span className="block truncate text-sm font-semibold">
            {entry.name}
          </span>
          <span className="mt-0.5 block truncate font-mono text-[11px] text-muted-foreground">
            {grid
              ? `${entry.flavor} · ${entry.version}`
              : `${entry.flavor} · ${entry.version} · ${entry.subtitle}`}
          </span>
        </motion.div>

        {status && (
          <motion.div
            layout="position"
            transition={layoutMorph}
            className={cn(grid && 'absolute top-2 left-2')}
          >
            <StatusBadge status={status} overlay={grid} />
          </motion.div>
        )}

        <motion.div
          layout="position"
          transition={layoutMorph}
          className={cn(
            grid &&
              'pointer-events-none absolute inset-x-0 top-0 aspect-square opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100 has-aria-expanded:opacity-100',
          )}
        >
          <div
            className={cn(
              '[&>*]:pointer-events-auto',
              grid && 'absolute right-3 bottom-3 grid',
            )}
          >
            <ActionButton entry={entry} square={grid} />
          </div>
        </motion.div>
      </MotionLink>
    </EntryContextMenu>
  );
}
