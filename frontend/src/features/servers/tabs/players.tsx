import { DotsThreeIcon, UsersIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';

import type { PlayerEntry, PlayerList } from '@/api';
import { Empty } from '@/components/empty';
import { SettingGroup, SettingList } from '@/components/setting-controls';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { FieldGroup } from '@/components/ui/field';
import { StatusDot } from '@/components/ui/status-dot';
import { AddRow, ValueRow } from '@/features/settings/components';
import { m } from '@/paraglide/messages.js';
import { playersMutations, playersQueries } from '@/queries/players';

/** One of the server's lists: its members, removable, and a row to add one. */
function ListGroup({
  legend,
  description,
  entries,
  editable,
  pending,
  onAdd,
  onRemove,
}: {
  legend: string;
  description: string;
  entries: PlayerEntry[];
  editable: boolean;
  pending: boolean;
  onAdd: (player: string) => void;
  onRemove: (player: string) => void;
}) {
  return (
    <SettingGroup legend={legend} description={description}>
      <SettingList>
        {entries.length === 0 && (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {m['server.players.none']()}
          </p>
        )}
        {entries.map((entry) => (
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
            pending={!editable || pending}
            onRemove={() => onRemove(entry.name)}
          />
        ))}
        {editable && (
          <AddRow
            placeholder={m['server.players.name_placeholder']()}
            label={m['content.add']()}
            pending={pending}
            onAdd={onAdd}
          />
        )}
      </SettingList>
    </SettingGroup>
  );
}

/**
 * Who is online and who is on the server's lists. The lists show for a stopped
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

  if (players.isPending) return <Bone className="h-64 max-w-2xl" />;
  if (!players.data) {
    return (
      <Empty className="h-full" icon={UsersIcon}>
        {m['server.players.unavailable']()}
      </Empty>
    );
  }

  const { online, whitelist, ops, banned } = players.data;
  const change = (list: PlayerList, listed: boolean) => (player: string) =>
    set.mutate({ player, list, listed });
  const isOp = (name: string) => ops.some((entry) => entry.name === name);

  return (
    <FieldGroup className="max-w-2xl gap-8">
      {!running && (
        <p className="text-xs text-muted-foreground">
          {m['server.players.stopped_hint']()}
        </p>
      )}

      {running && (
        <SettingGroup legend={m['server.players.online']()}>
          <SettingList>
            {online.length === 0 && (
              <p className="px-3 py-2 text-xs text-muted-foreground">
                {m['server.players.nobody_online']()}
              </p>
            )}
            {online.map((name) => (
              <div key={name} className="flex items-center gap-2 px-3 py-1.5">
                <StatusDot tone="on" />
                <span className="min-w-0 flex-1 truncate font-mono text-xs">
                  {name}
                </span>
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
                    <DropdownMenuItem
                      onClick={() => change('ops', !isOp(name))(name)}
                    >
                      {isOp(name)
                        ? m['server.players.action.deop']()
                        : m['server.players.action.op']()}
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => kick.mutate(name)}>
                      {m['server.players.action.kick']()}
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      variant="destructive"
                      onClick={() => change('banned', true)(name)}
                    >
                      {m['server.players.action.ban']()}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            ))}
          </SettingList>
        </SettingGroup>
      )}

      <ListGroup
        legend={m['server.players.whitelist']()}
        description={m['server.players.whitelist_hint']()}
        entries={whitelist}
        editable={running}
        pending={set.isPending}
        onAdd={change('whitelist', true)}
        onRemove={change('whitelist', false)}
      />
      <ListGroup
        legend={m['server.players.ops']()}
        description={m['server.players.ops_hint']()}
        entries={ops}
        editable={running}
        pending={set.isPending}
        onAdd={change('ops', true)}
        onRemove={change('ops', false)}
      />
      <ListGroup
        legend={m['server.players.banned']()}
        description={m['server.players.banned_hint']()}
        entries={banned}
        editable={running}
        pending={set.isPending}
        onAdd={change('banned', true)}
        onRemove={change('banned', false)}
      />
    </FieldGroup>
  );
}
