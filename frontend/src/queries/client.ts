/**
 * The one `QueryClient` the app runs on — a module singleton so the mutation
 * factories and the event-driven invalidation feed can reach it without a
 * React context.
 */
import {
  MutationCache,
  QueryCache,
  QueryClient,
  type QueryKey,
} from '@tanstack/react-query';
import { toast } from 'sonner';

import { logger } from '@/lib/log';
import {
  CONNECTION_LOST,
  errorMessage,
  type HestiaError,
  NOT_FOUND,
  OFFLINE,
  TIMEOUT,
  TRANSPORT,
  UNAUTHORIZED,
} from '../api';
import { scopedToEntry } from './keys';

const log = logger('query');

// Never a toast: `unauthorized` is expected behind the sign-in gate, and
// connectivity failures are the status bar's concern, not an error per
// background read. A page that needs upstream says so itself.
const TRANSPORT_CODES = new Set([TRANSPORT, CONNECTION_LOST, TIMEOUT, OFFLINE]);
const silent = (error: HestiaError) =>
  error.code === UNAUTHORIZED || TRANSPORT_CODES.has(error.code);

// A mutation is something the user asked for, so being offline is an answer
// they are owed — unlike a background read, where the status bar already says
// it. Only the daemon-side codes stay quiet.
const silentMutation = (error: HestiaError) =>
  error.code !== OFFLINE && silent(error);

// A read racing a removal — the entry went while the read was in flight, or a
// view still held its id — is absence, not a failure the user can act on.
const vanished = (error: HestiaError, key: QueryKey) =>
  error.code === NOT_FOUND && scopedToEntry(key);

declare module '@tanstack/react-query' {
  interface Register {
    defaultError: HestiaError;
  }
}

export const queryClient = new QueryClient({
  // Failures toast instead of rendering into pages; the query hash id keeps
  // a retriggering refetch replacing its own toast rather than stacking.
  queryCache: new QueryCache({
    onError: (error, query) => {
      const quiet = silent(error) || vanished(error, query.queryKey);
      log[quiet ? 'debug' : 'warn'](
        { key: query.queryHash, code: error.code },
        `query failed: ${error.message}`,
      );
      // A query may opt out of the toast (e.g. a ping a stopped server can't
      // answer) via meta.silent.
      if (query.meta?.silent || quiet) return;
      toast.error(errorMessage(error), { id: query.queryHash });
    },
  }),
  mutationCache: new MutationCache({
    onError: (error, _vars, _ctx, mutation) => {
      if (mutation.meta?.job) return;
      log.warn(
        { key: mutation.options.mutationKey?.join('.'), code: error.code },
        `mutation failed: ${error.message}`,
      );
      if (silentMutation(error)) return;
      toast.error(errorMessage(error));
    },
  }),
  defaultOptions: {
    queries: {
      // The daemon is a local socket, not HTTP: the webview's online/offline
      // signal is meaningless here, and failures are not transient network
      // blips worth retrying.
      networkMode: 'always',
      retry: false,
      // Refetching an errored query on mount loops: a data-less fetch reads as
      // `pending`, pages swap to skeletons, the observers remount, repeat.
      // Recovery is the reconnect sweep in ./invalidation.
      retryOnMount: false,
      // Daemon events invalidate what changes, so polling-style refetches
      // only need to catch what the topic map misses.
      staleTime: 30_000,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
    },
    mutations: {
      networkMode: 'always',
      retry: false,
    },
  },
});

/** Sweep every query under the key prefix. */
export function invalidate(key: QueryKey): void {
  void queryClient.invalidateQueries({ queryKey: key });
}
