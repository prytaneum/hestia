import { useMutation, useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import type { EntryKind } from '@/api';
import { useLaunch } from '@/features/instances/hooks';
import { contentQueries } from '@/queries/content';
import { instanceMutations, useInstances } from '@/queries/instance';
import { serverMutations, useServers } from '@/queries/server';
import {
  type EntryCardModel,
  instanceToCard,
  serverToCard,
} from '../components';

/** Each entry's count of known content updates, keyed `kind:id`. */
function useUpdateCounts(): (kind: EntryKind, id: string) => number {
  const { data } = useQuery(contentQueries.available());
  return useMemo(() => {
    const counts = new Map(
      (data ?? []).map((entry) => [
        `${entry.target.kind}:${entry.target.entry}`,
        entry.updates.filter((u) => u.updatable).length,
      ]),
    );
    return (kind, id) => counts.get(`${kind}:${id}`) ?? 0;
  }, [data]);
}

export function useServerCards(): {
  cards: EntryCardModel[];
  isPending: boolean;
} {
  const servers = useServers();
  const updates = useUpdateCounts();
  const start = useMutation(serverMutations.startAny());
  const stop = useMutation(serverMutations.stopAny());

  const busyId =
    start.isPending || stop.isPending
      ? ((start.variables ?? stop.variables) as string | undefined)
      : undefined;

  const cards = useMemo(
    () =>
      (servers.data ?? []).map((server) => ({
        ...serverToCard(server, {
          busy: busyId === server.id,
          onStart: () => start.mutate(server.id),
          onStop: () => stop.mutate(server.id),
        }),
        updates: updates('server', server.id),
      })),
    [servers.data, busyId, start, stop, updates],
  );
  return { cards, isPending: servers.isPending };
}

export function useInstanceCards(): {
  cards: EntryCardModel[];
  isPending: boolean;
} {
  const instances = useInstances();
  const updates = useUpdateCounts();
  const { launch, restart, isLaunching } = useLaunch();
  const stop = useMutation(instanceMutations.stopAny());

  const cards = useMemo(
    () =>
      (instances.data ?? []).map((instance) => {
        const stopping = stop.isPending && stop.variables?.id === instance.id;
        return {
          ...instanceToCard(instance, {
            busy: isLaunching(instance.id) || stopping,
            launching: isLaunching(instance.id),
            stopping,
            onStart: () => launch(instance.id),
            onStop: (session) => stop.mutate({ id: instance.id, session }),
            onNewSession: () => launch(instance.id, { newSession: true }),
            onRestart: (session) => restart(instance.id, session),
          }),
          updates: updates('instance', instance.id),
        };
      }),
    [instances.data, isLaunching, launch, restart, stop, updates],
  );
  return { cards, isPending: instances.isPending };
}
