import {
  MagnifyingGlassIcon,
  RowsIcon,
  SquaresFourIcon,
} from '@phosphor-icons/react';
import { AnimatePresence, motion } from 'motion/react';

import type { InstanceInfo, ServerInfo } from '@/api';
import { Empty } from '@/components/empty';
import type { FilterGroup } from '@/components/filter-menu';
import { Button } from '@/components/ui/button';
import { agoLabel } from '@/lib/format';
import { layoutMorph, listContainer } from '@/lib/motion';
import { runningSessions } from '@/lib/sessions';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { type EntryCardModel, EntryTile } from './entry-card';

export type View = 'grid' | 'list';

/** Quick-action wiring shared by every card builder. */
export type CardActions = {
  busy: boolean;
  onStart: () => void;
  /** `session` names one session to stop; absent stops the entry outright. */
  onStop: (session?: string) => void;
  /** Instances only: launch alongside the sessions already running. */
  onNewSession?: () => void;
  /** Instances only: stop one session and launch it again. */
  onRestart?: (session: string) => void;
  launching?: boolean;
  stopping?: boolean;
};

/** Map a live server record to the card shape, with its quick actions wired. */
export function serverToCard(
  server: ServerInfo & { iconUrl?: string },
  actions: CardActions,
): EntryCardModel {
  const running = server.process?.state === 'running';
  const address = `:${server.gamePort}`;
  const state = running ? m['app.status.online']() : m['app.status.stopped']();
  return {
    id: server.id,
    name: server.name,
    kind: 'server',
    flavor: server.flavor,
    version: server.gameVersion,
    iconUrl: server.iconUrl,
    running,
    ready: server.ready,
    subtitle: !server.ready
      ? m['app.status.preparing_ellipsis']()
      : `${address} · ${state}`,
    busy: actions.busy,
    onStart: actions.onStart,
    onStop: actions.onStop,
  };
}

/** Map a live instance record to the card shape, with its quick actions wired. */
export function instanceToCard(
  instance: InstanceInfo & { iconUrl?: string },
  actions: CardActions,
): EntryCardModel {
  const sessions = runningSessions(instance);
  return {
    id: instance.id,
    name: instance.name,
    kind: 'instance',
    flavor: instance.flavor,
    version: instance.gameVersion,
    iconUrl: instance.iconUrl,
    running: sessions.length > 0,
    ready: true,
    subtitle:
      sessions.length > 0
        ? m['entry.sessions_running']({ count: sessions.length })
        : instance.lastPlayedUnix
          ? `${m['app.label.last_played']()} ${agoLabel(instance.lastPlayedUnix)}`
          : m['app.status.stopped'](),
    sessions,
    busy: actions.busy,
    launching: actions.launching,
    stopping: actions.stopping,
    onStart: actions.onStart,
    onStop: actions.onStop,
    onNewSession: actions.onNewSession,
    onRestart: actions.onRestart,
  };
}

export function flavorsOf(cards: EntryCardModel[]): string[] {
  return [...new Set(cards.map((c) => c.flavor))];
}

export function filterCards(
  cards: EntryCardModel[],
  query: string,
  flavor: string = 'all',
) {
  const q = query.trim().toLowerCase();
  return cards.filter((c) => {
    if (flavor !== 'all' && c.flavor !== flavor) return false;
    if (!q) return true;
    return (
      c.name.toLowerCase().includes(q) ||
      c.flavor.toLowerCase().includes(q) ||
      c.version.toLowerCase().includes(q)
    );
  });
}

/** The flavor dimension of a card list: "All" plus every flavor present in it. */
export function flavorGroup(
  flavors: string[],
  value: string,
  onChange: (flavor: string) => void,
): FilterGroup {
  return {
    label: m['app.label.flavor'](),
    value,
    neutral: 'all',
    options: [
      { value: 'all', label: m['app.label.all']() },
      ...flavors.map((f) => ({
        value: f,
        label: f,
        className: 'capitalize',
      })),
    ],
    onChange,
  };
}

/** A single toggle that flips grid⇄list, showing the view you'll switch to. */
export function ViewToggle({
  view,
  onView,
}: {
  view: View;
  onView: (v: View) => void;
}) {
  const next: View = view === 'grid' ? 'list' : 'grid';
  const Icon = next === 'list' ? RowsIcon : SquaresFourIcon;
  const label =
    next === 'list'
      ? m['app.collection.switch_to_list']()
      : m['app.collection.switch_to_grid']();
  return (
    <Button
      variant="ghost"
      size="icon"
      aria-label={label}
      title={label}
      onClick={() => onView(next)}
      className="text-muted-foreground"
    >
      <Icon className="size-4" />
    </Button>
  );
}

export function EntryCollection({
  cards,
  view,
  empty,
}: {
  cards: EntryCardModel[];
  view: View;
  empty: string;
}) {
  if (cards.length === 0) {
    return <Empty icon={MagnifyingGlassIcon}>{empty}</Empty>;
  }
  return (
    <motion.div
      layout
      initial="hidden"
      animate="show"
      variants={listContainer(cards.length)}
      transition={layoutMorph}
      className={cn(
        view === 'list'
          ? 'divide-y divide-border border border-border'
          : 'grid gap-3 [grid-template-columns:repeat(auto-fill,minmax(10rem,1fr))]',
      )}
    >
      <AnimatePresence initial={false}>
        {cards.map((entry) => (
          <EntryTile key={entry.id} entry={entry} view={view} />
        ))}
      </AnimatePresence>
    </motion.div>
  );
}
