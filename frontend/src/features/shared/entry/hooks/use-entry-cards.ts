import { useMutation } from '@tanstack/react-query';
import { useMemo } from 'react';

import { useLaunch } from '@/features/instances/hooks';
import { instanceMutations, useInstances } from '@/queries/instance';
import { serverMutations, useServers } from '@/queries/server';
import {
  type EntryCardModel,
  instanceToCard,
  serverToCard,
} from '../components';

export function useServerCards(): {
  cards: EntryCardModel[];
  isPending: boolean;
} {
  const servers = useServers();
  const start = useMutation(serverMutations.startAny());
  const stop = useMutation(serverMutations.stopAny());

  const busyId =
    start.isPending || stop.isPending
      ? ((start.variables ?? stop.variables) as string | undefined)
      : undefined;

  const cards = useMemo(
    () =>
      (servers.data ?? []).map((server) =>
        serverToCard(server, {
          busy: busyId === server.id,
          onStart: () => start.mutate(server.id),
          onStop: () => stop.mutate(server.id),
        }),
      ),
    [servers.data, busyId, start, stop],
  );
  return { cards, isPending: servers.isPending };
}

export function useInstanceCards(): {
  cards: EntryCardModel[];
  isPending: boolean;
} {
  const instances = useInstances();
  const { launch, restart, isLaunching } = useLaunch();
  const stop = useMutation(instanceMutations.stopAny());

  const cards = useMemo(
    () =>
      (instances.data ?? []).map((instance) => {
        const stopping = stop.isPending && stop.variables?.id === instance.id;
        return instanceToCard(instance, {
          busy: isLaunching(instance.id) || stopping,
          launching: isLaunching(instance.id),
          stopping,
          onStart: () => launch(instance.id),
          onStop: (session) => stop.mutate({ id: instance.id, session }),
          onNewSession: () => launch(instance.id, { newSession: true }),
          onRestart: (session) => restart(instance.id, session),
        });
      }),
    [instances.data, isLaunching, launch, restart, stop],
  );
  return { cards, isPending: instances.isPending };
}
