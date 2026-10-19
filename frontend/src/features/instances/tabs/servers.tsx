import {
  ArrowsClockwiseIcon,
  ArrowsDownUpIcon,
  HardDrivesIcon,
  PlusIcon,
} from '@phosphor-icons/react';
import {
  useIsFetching,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import { useState } from 'react';

import type { InstanceInfo, ServerEntry } from '@/api';
import { Empty } from '@/components/empty';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Spinner } from '@/components/ui/spinner';
import { ServerReorderList, ServerRow } from '@/features/instances/components';
import { LaunchControl } from '@/features/instances/components/launch-control';
import { ServerEntryDialog } from '@/features/instances/dialogs';
import { TabToolbar } from '@/features/shared/entry/detail';
import { notify } from '@/lib/notify';
import { supportsQuickPlay } from '@/lib/quick-play';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { instanceMutations, instanceQueries } from '@/queries/instance';
import { keys } from '@/queries/keys';

/**
 * The instance's multiplayer list — the servers the in-game list shows, read
 * from its own `servers.dat`.
 *
 * The file belongs to the running game, which rewrites it wholesale on exit,
 * so an edit made while a session is open comes back with a warning saying so.
 * That is also why arranging is a mode with one write at the end rather than a
 * write per move.
 */
export function InstanceServersTab({ instance }: { instance: InstanceInfo }) {
  const client = useQueryClient();
  const servers = useQuery(instanceQueries.servers(instance.id));
  const remove = useMutation(instanceMutations.serverRemove(instance.id));
  const arrange = useMutation(instanceMutations.serversArrange(instance.id));
  const [editing, setEditing] = useState<ServerEntry | null | undefined>(
    undefined,
  );
  const [removing, setRemoving] = useState<ServerEntry | null>(null);
  const [order, setOrder] = useState<string[] | null>(null);

  const joinable = supportsQuickPlay(instance.gameVersion);
  const running = runningSessions(instance).length > 0;
  // The game's own scratch rows (direct-connect) are not the player's servers.
  const list = (servers.data ?? []).filter((server) => !server.hidden);
  const arranging = order !== null;

  const pinging = useIsFetching({ queryKey: keys.instances.serverStatuses() });
  const refresh = () => {
    client.invalidateQueries({ queryKey: keys.instances.servers(instance.id) });
    client.invalidateQueries({ queryKey: keys.instances.serverStatuses() });
  };

  const commit = () => {
    if (!order) return;
    arrange.mutate(order, {
      onSuccess: (written) => {
        notify.warnings(written.warnings, { id: 'servers-list:write' });
        setOrder(null);
      },
    });
  };

  return (
    <div className="flex flex-1 flex-col">
      <TabToolbar className="justify-between gap-3">
        <p className="text-sm text-muted-foreground">
          {arranging
            ? m['instance.servers.reorder.hint']()
            : m['instance.servers.summary']()}
        </p>
        <div className="flex shrink-0 items-center gap-2">
          {arranging ? (
            <>
              <Button
                size="sm"
                variant="ghost"
                disabled={arrange.isPending}
                onClick={() => setOrder(null)}
              >
                {m['app.action.cancel']()}
              </Button>
              <Button size="sm" disabled={arrange.isPending} onClick={commit}>
                {arrange.isPending ? <Spinner /> : m['app.action.done']()}
              </Button>
            </>
          ) : (
            <>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label={m['app.action.refresh']()}
                title={m['app.action.refresh']()}
                disabled={servers.isPending || pinging > 0}
                onClick={refresh}
              >
                <ArrowsClockwiseIcon
                  className={pinging > 0 ? 'animate-spin' : undefined}
                />
              </Button>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label={m['app.action.reorder']()}
                title={m['app.action.reorder']()}
                disabled={list.length < 2}
                onClick={() => setOrder(list.map((s) => s.name || s.address))}
              >
                <ArrowsDownUpIcon />
              </Button>
              <Button
                size="sm"
                data-icon="inline-start"
                onClick={() => setEditing(null)}
              >
                <PlusIcon weight="bold" />
                {m['instance.servers.add.action']()}
              </Button>
            </>
          )}
        </div>
      </TabToolbar>

      {servers.isPending ? (
        <div className="space-y-2">
          <Bone className="h-13" />
          <Bone className="h-13" />
        </div>
      ) : list.length === 0 ? (
        <Empty className="flex-1" icon={HardDrivesIcon}>
          {m['instance.servers.empty']()}
        </Empty>
      ) : arranging ? (
        <ServerReorderList servers={list} onOrder={setOrder} />
      ) : (
        <div className="divide-y divide-border border border-border">
          {list.map((server) => (
            <ServerRow
              key={`${server.name}:${server.address}`}
              server={server}
              play={
                <LaunchControl
                  instanceId={instance.id}
                  size="sm"
                  label="join"
                  quickPlay={{ kind: 'server', target: server.address }}
                  newSession={running}
                  disabled={!joinable}
                  disabledReason={m['instance.quick_play.unsupported']()}
                />
              }
              onEdit={() => setEditing(server)}
              onRemove={() => setRemoving(server)}
            />
          ))}
        </div>
      )}

      <ServerEntryDialog
        instance={instance.id}
        server={editing}
        onOpenChange={(open) => {
          if (!open) setEditing(undefined);
        }}
      />
      <ConfirmDialog
        open={removing != null}
        onOpenChange={(open) => {
          if (!open) setRemoving(null);
        }}
        title={m['instance.servers.remove.title']({
          name: removing?.name ?? '',
        })}
        description={m['instance.servers.remove.description']()}
        confirmLabel={m['app.action.remove']()}
        destructive
        onConfirm={() => {
          const target = removing;
          if (!target) return;
          remove.mutate(target.name || target.address, {
            onSuccess: (written) =>
              notify.warnings(written.warnings, { id: 'servers-list:write' }),
          });
          setRemoving(null);
        }}
      />
    </div>
  );
}
