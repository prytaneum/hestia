import { useQuery } from '@tanstack/react-query';
import { instanceQueries } from './instance';
import type { JobEntryKind } from './jobs';
import { serverQueries } from './server';

export function useEntryVersions(
  kind: JobEntryKind,
  flavor: string,
  enabled = true,
) {
  const server = useQuery({
    ...serverQueries.versions(flavor),
    enabled: enabled && kind === 'server',
  });
  const instance = useQuery({
    ...instanceQueries.versions(flavor),
    enabled: enabled && kind === 'instance',
  });
  return kind === 'server' ? server : instance;
}
