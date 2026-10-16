/** `net.*` — reachability as a query seeded once and kept live by its topic. */
import { queryOptions, useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';

import type { NetworkStatus } from '../api';
import { onTopic } from '../api/core/events';
import * as api from '../api/net';
import { queryClient } from './client';
import { useConnection } from './connection';
import { keys } from './keys';

const netQueries = {
  status: () =>
    queryOptions({
      queryKey: keys.net.status(),
      queryFn: () => api.status(),
    }),
};

/**
 * Reachability, live. Read once on connect, then written straight into the
 * cache by every `net.state` push — the daemon already watches this, so a
 * poll here would only duplicate its probe on a worse schedule.
 */
export function useNetwork(): NetworkStatus | undefined {
  const connected = useConnection() === 'connected';
  const query = useQuery({ ...netQueries.status(), enabled: connected });

  useEffect(() => {
    const off = onTopic<NetworkStatus>('net.state', (status) => {
      queryClient.setQueryData(keys.net.status(), status);
    });
    return () => {
      off.then((unsubscribe) => unsubscribe()).catch(() => {});
    };
  }, []);

  return query.data;
}

/** Whether the launcher cannot reach upstream right now. */
export function useOffline(): boolean {
  return useNetwork()?.state === 'offline';
}
