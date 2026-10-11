import {
  CubeIcon,
  HardDrivesIcon,
  PlayIcon,
  PlusIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Reorder, type Transition } from 'motion/react';
import { useEffect, useMemo, useRef, useState } from 'react';

import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { StatusDot } from '@/components/ui/status-dot';
import { Thumbnail } from '@/components/ui/thumbnail';
import { useLaunchDialog } from '@/features/instances/dialogs';
import { EntryContextMenu } from '@/features/shared/entry/components';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useInstances } from '@/queries/instance';
import { type PinnedEntry, pinKey, usePinned } from '@/queries/pinned';
import { serverMutations, serverQueries, useServers } from '@/queries/server';

type ResolvedPin = PinnedEntry & {
  name: string;
  flavor: string;
  version: string;
  running: boolean;
  /** A server still provisioning cannot start; an instance is always ready. */
  ready: boolean;
  /** Running session count — instances only; a server is always 0. */
  sessions: number;
  iconUrl?: string;
};

/**
 * A sidebar row travels one row-height, so Motion's 450ms layout default
 * reads as lag. Scoped to `layout` to leave the lift's own spring alone.
 */
const REORDER_TRANSITION: Transition = {
  layout: { duration: 0.15, ease: [0.2, 0, 0, 1] },
};

/**
 * Reorder state for the pin list.
 *
 * `Reorder` reports a new order on every crossing, so the drag runs against a
 * local preview keyed by `pinKey` and only the release writes to prefs — one
 * pref write per gesture rather than one per row swap. Keys rather than rows
 * keep the preview from pinning stale run state while the write settles; it
 * clears once prefs agree, and drops if the pins change underneath.
 */
function usePinnedReorder(
  pinned: ResolvedPin[],
  save: (entries: PinnedEntry[]) => void,
) {
  const [order, setOrder] = useState<string[] | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const suppressClick = useRef(false);

  const rows = useMemo(() => {
    if (!order) return pinned;
    const byKey = new Map(pinned.map((pin) => [pinKey(pin), pin]));
    const preview = order.flatMap((key) => byKey.get(key) ?? []);
    return preview.length === pinned.length ? preview : pinned;
  }, [order, pinned]);

  useEffect(() => {
    if (order && order.join() === pinned.map(pinKey).join()) setOrder(null);
  }, [order, pinned]);

  return {
    rows,
    keys: useMemo(() => rows.map(pinKey), [rows]),
    reorder: setOrder,
    isDragging: (key: string) => dragging === key,
    // Motion only reports a drag once it passes its own threshold, so a plain
    // click still navigates; the one ending a drag must not.
    onDragStart: (key: string) => {
      suppressClick.current = true;
      setDragging(key);
    },
    onDragEnd: () => {
      setDragging(null);
      // A drag that ends where it started leaves the effect above to clear the
      // preview; writing an unchanged order would be a pref write for nothing.
      if (rows.map(pinKey).join() === pinned.map(pinKey).join()) return;
      save(rows.map(({ kind, id }) => ({ kind, id })));
    },
    onClickCapture: (event: React.MouseEvent) => {
      if (!suppressClick.current) return;
      suppressClick.current = false;
      event.preventDefault();
      event.stopPropagation();
    },
    // A drag ending off the row raises no click to clear the flag on.
    onPointerDown: () => {
      suppressClick.current = false;
    },
  };
}

export function PinnedSection({ pathname }: { pathname: string }) {
  const instances = useInstances();
  const servers = useServers();
  const { pins: pinnedEntries, ready, isPinned, toggle, save } = usePinned();

  const instanceList = instances.data ?? [];
  const serverList = servers.data ?? [];

  const pinned = useMemo<ResolvedPin[]>(
    () =>
      pinnedEntries.flatMap((pin) => {
        if (pin.kind === 'instance') {
          const entry = instanceList.find((i) => i.id === pin.id);
          if (!entry) return [];
          const sessions = (entry.sessions ?? []).filter(
            (session) => session.state === 'running',
          ).length;
          return [
            {
              ...pin,
              name: entry.name,
              flavor: entry.flavor,
              version: entry.gameVersion,
              running: sessions > 0,
              ready: true,
              sessions,
              iconUrl: entry.iconUrl,
            },
          ];
        }
        const entry = serverList.find((s) => s.id === pin.id);
        if (!entry) return [];
        return [
          {
            ...pin,
            name: entry.name,
            flavor: entry.flavor,
            version: entry.gameVersion,
            running: entry.process?.state === 'running',
            ready: entry.ready,
            sessions: 0,
            iconUrl: entry.iconUrl,
          },
        ];
      }),
    [pinnedEntries, instanceList, serverList],
  );

  // Persist the pruned list when a pinned entry is deleted elsewhere. Both
  // lists must be loaded first, or a still-fetching query reads as empty and
  // would wrongly drop live pins. The ref keeps the effect off `save`'s churn.
  const saveRef = useRef(save);
  saveRef.current = save;
  useEffect(() => {
    if (!ready || !instances.data || !servers.data) return;
    if (pinned.length === pinnedEntries.length) return;
    saveRef.current(pinned.map(({ kind, id }) => ({ kind, id })));
  }, [ready, instances.data, servers.data, pinned, pinnedEntries]);

  const { launch, isLaunching } = useLaunchDialog();
  const startServer = useMutation(serverMutations.startAny());
  const play = (pin: PinnedEntry) => {
    if (pin.kind === 'server') return startServer.mutate(pin.id);
    const instance = instanceList.find((i) => i.id === pin.id);
    if (instance) launch(instance);
  };
  const isStarting = (pin: PinnedEntry) =>
    pin.kind === 'server'
      ? startServer.isPending && startServer.variables === pin.id
      : isLaunching(pin.id);

  const reorder = usePinnedReorder(pinned, save);
  const nothingToPin = instanceList.length === 0 && serverList.length === 0;

  return (
    <div className="border-t border-border p-2">
      <div className="flex items-center justify-between px-3 pt-1 pb-1.5">
        <span className="text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
          {m['app.label.pinned']()}
        </span>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <button
                type="button"
                aria-label={m['app.label.pin_entries']()}
                title={m['app.label.pin_entries']()}
                disabled={
                  !ready ||
                  instances.isPending ||
                  servers.isPending ||
                  nothingToPin
                }
                className="text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
              >
                <PlusIcon className="size-3.5" />
              </button>
            }
          />
          <DropdownMenuContent align="end" className="w-52">
            {instanceList.length > 0 && (
              <DropdownMenuGroup>
                <DropdownMenuLabel>
                  {m['app.nav.instances']()}
                </DropdownMenuLabel>
                {instanceList.map((instance) => (
                  <DropdownMenuCheckboxItem
                    key={instance.id}
                    checked={isPinned({ kind: 'instance', id: instance.id })}
                    onCheckedChange={() =>
                      toggle({ kind: 'instance', id: instance.id })
                    }
                  >
                    {instance.name}
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuGroup>
            )}
            {serverList.length > 0 && (
              <DropdownMenuGroup>
                <DropdownMenuLabel>{m['app.nav.servers']()}</DropdownMenuLabel>
                {serverList.map((server) => (
                  <DropdownMenuCheckboxItem
                    key={server.id}
                    checked={isPinned({ kind: 'server', id: server.id })}
                    onCheckedChange={() =>
                      toggle({ kind: 'server', id: server.id })
                    }
                  >
                    {server.name}
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuGroup>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {pinned.length === 0 ? (
        <p className="px-3 py-1.5 text-[11px] text-muted-foreground/70">
          {m['app.label.nothing_pinned']()}
        </p>
      ) : (
        <Reorder.Group
          as="div"
          axis="y"
          values={reorder.keys}
          onReorder={reorder.reorder}
          className="flex flex-col gap-0.5"
        >
          {reorder.rows.map((entry) => (
            <PinnedRow
              key={pinKey(entry)}
              entry={entry}
              pathname={pathname}
              reorder={reorder}
              starting={isStarting(entry)}
              onPlay={() => play(entry)}
            />
          ))}
        </Reorder.Group>
      )}
    </div>
  );
}

function PinnedRow({
  entry,
  pathname,
  reorder,
  starting,
  onPlay,
}: {
  entry: ResolvedPin;
  pathname: string;
  reorder: ReturnType<typeof usePinnedReorder>;
  starting: boolean;
  onPlay: () => void;
}) {
  const key = pinKey(entry);
  const dragging = reorder.isDragging(key);
  const playable = entry.ready && !entry.running;
  const playLabel =
    entry.kind === 'server' ? m['app.action.start']() : m['app.action.play']();

  return (
    <Reorder.Item
      as="div"
      value={key}
      transition={REORDER_TRANSITION}
      dragMomentum={false}
      whileDrag={{ scale: 1.02 }}
      onDragStart={() => reorder.onDragStart(key)}
      onDragEnd={reorder.onDragEnd}
      onPointerDown={reorder.onPointerDown}
      onClickCapture={reorder.onClickCapture}
      className="group/pin relative"
    >
      <EntryContextMenu entry={entry}>
        <PinnedLink
          entry={entry}
          pathname={pathname}
          dragging={dragging}
          className={cn(
            playable && 'group-focus-within/pin:pr-8 group-hover/pin:pr-8',
          )}
        />
      </EntryContextMenu>
      {/* Beside the link, not in it: a button nested in an anchor is invalid. */}
      {playable && (
        <button
          type="button"
          aria-label={playLabel}
          title={playLabel}
          disabled={starting}
          onClick={onPlay}
          className="absolute top-1/2 right-2.5 grid size-5 -translate-y-1/2 place-items-center text-muted-foreground opacity-0 transition-[opacity,color] outline-none group-hover/pin:opacity-100 hover:text-ember focus-visible:opacity-100 focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-40"
        >
          <PlayIcon weight="fill" className="size-3" />
        </button>
      )}
    </Reorder.Item>
  );
}

/** Spreads what it is not given onto the link, which the menu trigger relies on. */
function PinnedLink({
  entry,
  pathname,
  dragging,
  className: triggerClass,
  ...trigger
}: {
  entry: ResolvedPin;
  pathname: string;
  dragging: boolean;
} & React.ComponentProps<'a'>) {
  const active = pathname === `/${entry.kind}s/${entry.id}`;
  const content = <PinnedLinkContent entry={entry} />;
  const className = cn(pinnedLinkClass(active, dragging), triggerClass);

  if (entry.kind === 'server') {
    return (
      <Link
        {...trigger}
        to="/servers/$id"
        params={{ id: entry.id }}
        className={className}
      >
        {content}
      </Link>
    );
  }

  return (
    <Link
      {...trigger}
      to="/instances/$id"
      params={{ id: entry.id }}
      className={className}
    >
      {content}
    </Link>
  );
}

function pinnedLinkClass(active: boolean, dragging: boolean) {
  return cn(
    'flex items-center gap-2.5 px-3 py-1.5 transition-colors outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset',
    active
      ? 'bg-muted text-foreground'
      : 'text-muted-foreground group-hover/pin:bg-muted/60',
    dragging && 'bg-muted shadow-lg',
  );
}

function PinnedLinkContent({ entry }: { entry: ResolvedPin }) {
  const Icon = entry.kind === 'server' ? HardDrivesIcon : CubeIcon;
  return (
    <>
      <Thumbnail src={entry.iconUrl} icon={Icon} size="xs" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-xs text-foreground">
          {entry.name}
        </span>
        <span className="block truncate font-mono text-[10px] text-muted-foreground">
          {entry.flavor} · {entry.version}
        </span>
      </span>
      <span className="flex shrink-0 items-center gap-1.5">
        {entry.running && entry.kind === 'server' && (
          <ServerPinPlayers id={entry.id} />
        )}
        {entry.running && entry.kind === 'instance' && entry.sessions > 1 && (
          <span
            className="font-mono text-[10px]"
            title={m['entry.sessions_running']({ count: entry.sessions })}
          >
            ×{entry.sessions}
          </span>
        )}
        {entry.running && <StatusDot tone="on" />}
      </span>
    </>
  );
}

/** A running server's live player count, polled only while the pin is mounted. */
function ServerPinPlayers({ id }: { id: string }) {
  const ping = useQuery(serverQueries.ping(id));
  if (!ping.data) return null;
  return (
    <span className="font-mono text-[10px]" title={m['app.label.players']()}>
      {ping.data.playersOnline}/{ping.data.playersMax}
    </span>
  );
}
