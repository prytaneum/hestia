import {
  ArrowCounterClockwiseIcon,
  DotsThreeIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  ProhibitIcon,
  ShieldStarIcon,
  SignOutIcon,
  TrashIcon,
  UsersIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { type ReactNode, useState } from 'react';

import type { PlayerEntry, PlayerList } from '@/api';
import { AccountAvatar } from '@/components/app-shell/account-avatar';
import { chipClass } from '@/components/chip';
import { Empty } from '@/components/empty';
import { SearchInput } from '@/components/search-input';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { StatusDot } from '@/components/ui/status-dot';
import { TabToolbar } from '@/features/shared/entry/detail';
import { m } from '@/paraglide/messages.js';
import { playersMutations, playersQueries } from '@/queries/players';

type View = 'online' | PlayerList;

const viewInfo: Record<View, { label: () => string; hint?: () => string }> = {
  online: { label: m['server.players.online'] },
  whitelist: {
    label: m['server.players.whitelist'],
    hint: m['server.players.whitelist_hint'],
  },
  ops: { label: m['server.players.ops'], hint: m['server.players.ops_hint'] },
  banned: {
    label: m['server.players.banned'],
    hint: m['server.players.banned_hint'],
  },
};

function PlayerRow({
  player,
  online,
  op,
  disabled,
  children,
}: {
  player: PlayerEntry;
  online: boolean;
  op: boolean;
  disabled: boolean;
  children: ReactNode;
}) {
  const detail = [player.uuid, player.reason].filter(Boolean).join(' · ');
  return (
    <div className="flex items-center gap-3 px-3 py-2.5">
      <AccountAvatar uuid={player.uuid} name={player.name} size={32} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          {online && <StatusDot tone="on" />}
          <span className="truncate text-sm">{player.name}</span>
          {op && (
            <Badge variant="outline" className="shrink-0">
              {m['server.players.operator']()}
            </Badge>
          )}
        </div>
        {detail && (
          <div className="truncate font-mono text-[11px] text-muted-foreground">
            {detail}
          </div>
        )}
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="ghost"
              size="icon-sm"
              className="shrink-0"
              disabled={disabled}
              aria-label={m['server.players.actions_for']({
                name: player.name,
              })}
            >
              <DotsThreeIcon weight="bold" className="size-4" />
            </Button>
          }
        />
        <DropdownMenuContent align="end" className="w-52">
          {children}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function AddPlayer({
  pending,
  onAdd,
}: {
  pending: boolean;
  onAdd: (player: string) => void;
}) {
  const [draft, setDraft] = useState('');
  const name = draft.trim();

  const add = () => {
    if (!name) return;
    setDraft('');
    onAdd(name);
  };

  return (
    <div className="ml-auto flex items-center gap-2">
      <Input
        value={draft}
        placeholder={m['server.players.name_placeholder']()}
        className="w-48"
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') add();
        }}
      />
      <Button
        size="sm"
        variant="outline"
        data-icon="inline-start"
        disabled={pending || !name}
        onClick={add}
      >
        <PlusIcon weight="bold" />
        {m['server.players.add']()}
      </Button>
    </div>
  );
}

/**
 * Who is online and who is on the server's lists, one list at a time so a long
 * one never pushes the others off the page. The lists show for a stopped
 * server; changing one is a console command, so it waits for the server.
 */
export function ServerPlayersTab({
  id,
  running,
}: {
  id: string;
  running: boolean;
}) {
  const players = useQuery(playersQueries.list(id, running));
  const set = useMutation(playersMutations.set(id));
  const kick = useMutation(playersMutations.kick(id));
  const [chosen, setChosen] = useState<View>('online');
  const [search, setSearch] = useState('');

  if (players.isPending) return <Bone className="h-64" />;
  if (!players.data) {
    return (
      <Empty className="h-full" icon={UsersIcon}>
        {m['server.players.unavailable']()}
      </Empty>
    );
  }

  const { online, whitelist, ops, banned } = players.data;
  const lists: Record<View, PlayerEntry[]> = {
    online,
    whitelist,
    ops,
    banned,
  };
  const views: View[] = running
    ? ['online', 'whitelist', 'ops', 'banned']
    : ['whitelist', 'ops', 'banned'];
  const view = views.includes(chosen) ? chosen : 'whitelist';

  const entries = lists[view];
  const needle = search.trim().toLowerCase();
  const shown = entries.filter((entry) =>
    entry.name.toLowerCase().includes(needle),
  );

  const onlineNames = new Set(online.map((entry) => entry.name));
  const opNames = new Set(ops.map((entry) => entry.name));
  const change = (list: PlayerList, listed: boolean) => (player: string) =>
    set.mutate({ player, list, listed });

  const opItem = (name: string) => (
    <DropdownMenuItem onClick={() => change('ops', !opNames.has(name))(name)}>
      <ShieldStarIcon />
      {opNames.has(name)
        ? m['server.players.action.deop']()
        : m['server.players.action.op']()}
    </DropdownMenuItem>
  );
  const actions: Record<View, (name: string) => ReactNode> = {
    online: (name) => (
      <>
        {opItem(name)}
        <DropdownMenuItem onClick={() => kick.mutate(name)}>
          <SignOutIcon />
          {m['server.players.action.kick']()}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          variant="destructive"
          onClick={() => change('banned', true)(name)}
        >
          <ProhibitIcon />
          {m['server.players.action.ban']()}
        </DropdownMenuItem>
      </>
    ),
    whitelist: (name) => (
      <>
        {opItem(name)}
        <DropdownMenuSeparator />
        <DropdownMenuItem
          variant="destructive"
          onClick={() => change('whitelist', false)(name)}
        >
          <TrashIcon />
          {m['server.players.action.unwhitelist']()}
        </DropdownMenuItem>
      </>
    ),
    ops: opItem,
    banned: (name) => (
      <DropdownMenuItem onClick={() => change('banned', false)(name)}>
        <ArrowCounterClockwiseIcon />
        {m['server.players.action.pardon']()}
      </DropdownMenuItem>
    ),
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <TabToolbar>
        <SearchInput
          value={search}
          onChange={setSearch}
          placeholder={m['app.search.players']()}
          className="w-56"
          delay={0}
        />
        {running && view !== 'online' && (
          <AddPlayer pending={set.isPending} onAdd={change(view, true)} />
        )}
      </TabToolbar>

      <div className="mb-3 flex flex-wrap items-center gap-1.5">
        {views.map((v) => (
          <button
            key={v}
            type="button"
            aria-pressed={view === v}
            className={chipClass(view === v)}
            onClick={() => setChosen(v)}
          >
            {viewInfo[v].label()}
            <span className="ml-1.5 font-mono text-[10px] opacity-60">
              {lists[v].length}
            </span>
          </button>
        ))}
        {running && (
          <span className="ml-auto truncate text-xs text-muted-foreground">
            {viewInfo[view].hint?.()}
          </span>
        )}
      </div>

      {!running && (
        <p className="mb-3 text-xs text-muted-foreground">
          {m['server.players.stopped_hint']()}
        </p>
      )}

      {shown.length > 0 ? (
        <div className="min-h-0 divide-y divide-border overflow-y-auto border border-border">
          {shown.map((entry) => (
            <PlayerRow
              key={entry.uuid || entry.name}
              player={entry}
              online={onlineNames.has(entry.name)}
              op={view !== 'ops' && opNames.has(entry.name)}
              disabled={!running || set.isPending}
            >
              {actions[view](entry.name)}
            </PlayerRow>
          ))}
        </div>
      ) : entries.length > 0 ? (
        <Empty className="flex-1" icon={MagnifyingGlassIcon}>
          {m['server.players.none_match']()}
        </Empty>
      ) : (
        <Empty className="flex-1" icon={UsersIcon}>
          {view === 'online'
            ? m['server.players.nobody_online']()
            : m['server.players.none']()}
        </Empty>
      )}
    </div>
  );
}
