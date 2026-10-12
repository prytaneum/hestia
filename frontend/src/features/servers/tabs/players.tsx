import {
  DotsThreeIcon,
  MagnifyingGlassIcon,
  UsersIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import type { PlayerEntry, PlayerList } from '@/api';
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
import { StatusDot } from '@/components/ui/status-dot';
import { AddRow, ValueRow } from '@/features/settings/components';
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

const matches = (name: string, search: string) =>
  name.toLowerCase().includes(search.trim().toLowerCase());

function OnlineRow({
  name,
  op,
  onOp,
  onKick,
  onBan,
}: {
  name: string;
  op: boolean;
  onOp: () => void;
  onKick: () => void;
  onBan: () => void;
}) {
  return (
    <div className="flex items-center gap-2 px-3 py-1.5">
      <StatusDot tone="on" />
      <span className="min-w-0 flex-1 truncate font-mono text-xs">{name}</span>
      {op && <Badge variant="outline">{m['server.players.operator']()}</Badge>}
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={m['server.players.actions_for']({ name })}
            >
              <DotsThreeIcon weight="bold" className="size-4" />
            </Button>
          }
        />
        <DropdownMenuContent align="end" className="w-44">
          <DropdownMenuItem onClick={onOp}>
            {op
              ? m['server.players.action.deop']()
              : m['server.players.action.op']()}
          </DropdownMenuItem>
          <DropdownMenuItem onClick={onKick}>
            {m['server.players.action.kick']()}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onClick={onBan}>
            {m['server.players.action.ban']()}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
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

  if (players.isPending) return <Bone className="h-64 max-w-2xl" />;
  if (!players.data) {
    return (
      <Empty className="h-full" icon={UsersIcon}>
        {m['server.players.unavailable']()}
      </Empty>
    );
  }

  const { online, whitelist, ops, banned } = players.data;
  const lists: Record<PlayerList, PlayerEntry[]> = { whitelist, ops, banned };
  const views: View[] = running
    ? ['online', 'whitelist', 'ops', 'banned']
    : ['whitelist', 'ops', 'banned'];
  const view = views.includes(chosen) ? chosen : 'whitelist';
  const total = view === 'online' ? online.length : lists[view].length;
  const hint = running
    ? viewInfo[view].hint?.()
    : m['server.players.stopped_hint']();

  const change = (list: PlayerList, listed: boolean) => (player: string) =>
    set.mutate({ player, list, listed });
  const opNames = new Set(ops.map((entry) => entry.name));

  const rows =
    view === 'online'
      ? online
          .filter((name) => matches(name, search))
          .map((name) => (
            <OnlineRow
              key={name}
              name={name}
              op={opNames.has(name)}
              onOp={() => change('ops', !opNames.has(name))(name)}
              onKick={() => kick.mutate(name)}
              onBan={() => change('banned', true)(name)}
            />
          ))
      : lists[view]
          .filter((entry) => matches(entry.name, search))
          .map((entry) => (
            <ValueRow
              key={entry.uuid || entry.name}
              value={entry.name}
              badge={
                entry.reason && (
                  <span className="truncate text-[11px] text-muted-foreground">
                    {entry.reason}
                  </span>
                )
              }
              pending={!running || set.isPending}
              onRemove={() => change(view, false)(entry.name)}
            />
          ));

  return (
    <div className="flex min-h-0 max-w-2xl flex-1 flex-col">
      <div className="mb-3 flex items-center gap-2">
        <SearchInput
          value={search}
          onChange={setSearch}
          placeholder={m['app.search.players']()}
          className="w-56"
          delay={0}
        />
        {running && view !== 'online' && (
          <div className="ml-auto w-64 border border-border">
            <AddRow
              placeholder={m['server.players.name_placeholder']()}
              label={m['server.players.add']()}
              pending={set.isPending}
              onAdd={change(view, true)}
            />
          </div>
        )}
      </div>

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
              {v === 'online' ? online.length : lists[v].length}
            </span>
          </button>
        ))}
      </div>

      {hint && <p className="mb-3 text-xs text-muted-foreground">{hint}</p>}

      {rows.length > 0 ? (
        <div className="min-h-0 divide-y divide-border overflow-y-auto border border-border">
          {rows}
        </div>
      ) : total > 0 ? (
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
