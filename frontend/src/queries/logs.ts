/**
 * Live log following, shared by the server/instance/process log hooks: a
 * fetched tail (the query) plus `process.output` events accumulated on top.
 * The buffer resets whenever a fresh tail lands, so a refetch never
 * duplicates lines it already contains.
 *
 * The subject is the entry, not one process: the matcher accepts the entry's
 * whole process family, so a stop leaves the stream in place and the next
 * start resumes it without a remount.
 */
import type { UseQueryResult } from '@tanstack/react-query';
import { useMemo, useRef, useState } from 'react';
import type { HestiaError, ProcessLogLine } from '../api';
import { useDaemonEvent } from './events';

export interface LogsOptions {
  /** How many lines of history to fetch. */
  tail?: number;
  /** Accumulate `process.output` events on top of the fetched tail. */
  follow?: boolean;
  /** Cap on accumulated live lines; default 1000. */
  limit?: number;
}

export type LogsResult = UseQueryResult<ProcessLogLine[], HestiaError> & {
  /** The fetched tail plus everything streamed since. */
  lines: ProcessLogLine[];
};

// The daemon coalesces a tail poll's lines into one event, so the wire shape is
// `{ id, lines: ProcessLogLine[] }` — a batch, not a single line.
interface ProcessOutputPayload {
  id: string;
  lines: ProcessLogLine[];
}

export function useFollowedLogs(
  query: UseQueryResult<ProcessLogLine[], HestiaError>,
  matches: ((processId: string) => boolean) | null,
  limit = 1000,
): LogsResult {
  const [live, setLive] = useState<ProcessLogLine[]>([]);
  const matchesRef = useRef(matches);
  matchesRef.current = matches;
  const following = matches !== null;

  // Following, keep the buffer across refetches — a background tail refetch
  // wiping it would drop lines newer than its window (a visible gap); the seam
  // is de-duplicated below. Not following, reset so a refetch never duplicates.
  const [seenFetchAt, setSeenFetchAt] = useState(query.dataUpdatedAt);
  if (seenFetchAt !== query.dataUpdatedAt) {
    setSeenFetchAt(query.dataUpdatedAt);
    if (!following) setLive([]);
  }

  // A new process writes a fresh log (the game rotates `latest.log` at
  // startup), so the accumulated buffer belongs to the run that just ended:
  // drop it and let the refetched tail be the new run's history.
  useDaemonEvent<{ id: string }>('process.started', (payload) => {
    if (matchesRef.current?.(payload.id)) setLive([]);
  });

  useDaemonEvent<ProcessOutputPayload>('process.output', (payload) => {
    if (!matchesRef.current?.(payload.id) || payload.lines.length === 0) return;
    setLive((lines) => {
      const merged = [...lines, ...payload.lines];
      return merged.length > limit
        ? merged.slice(merged.length - limit)
        : merged;
    });
  });

  const lines = useMemo(() => {
    const base = query.data ?? [];
    const tail = following ? dropLeadingOverlap(base, live) : live;
    return [...base, ...tail];
  }, [query.data, live, following]);
  return { ...query, lines };
}

/** Drop the longest prefix of `live` that already ends `base` (same content). */
function dropLeadingOverlap(
  base: ProcessLogLine[],
  live: ProcessLogLine[],
): ProcessLogLine[] {
  const max = Math.min(base.length, live.length);
  for (let k = max; k > 0; k--) {
    let match = true;
    for (let i = 0; i < k; i++) {
      const b = base[base.length - k + i];
      const l = live[i];
      if (b.offset !== l.offset || b.line !== l.line || b.stream !== l.stream) {
        match = false;
        break;
      }
    }
    if (match) return live.slice(k);
  }
  return live;
}
