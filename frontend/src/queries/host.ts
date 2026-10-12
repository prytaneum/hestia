/** `host.*` — the daemon's machine, and the memory range it can honour. */
import { queryOptions, useQuery } from '@tanstack/react-query';
import * as api from '../api/host';
import { keys } from './keys';

const BYTES_PER_GB = 1024 ** 3;
const MIN_MEMORY_GB = 2;
/** The ceiling offered until the host answers, or when it reports no memory. */
const UNKNOWN_HOST_MEMORY_GB = 32;

export const hostQueries = {
  info: () =>
    queryOptions({
      queryKey: keys.host.info(),
      queryFn: () => api.info(),
      staleTime: Number.POSITIVE_INFINITY,
    }),
};

export interface MemoryRange {
  min: number;
  max: number;
}

/**
 * The whole-gigabyte range a memory control offers. The ceiling is rounded
 * down because the daemon rejects anything above the installed amount.
 */
export function useMemoryRange(): MemoryRange {
  const total = useQuery(hostQueries.info()).data?.totalMemoryBytes;
  const max = total ? Math.floor(total / BYTES_PER_GB) : UNKNOWN_HOST_MEMORY_GB;
  return { min: MIN_MEMORY_GB, max: Math.max(MIN_MEMORY_GB, max) };
}
