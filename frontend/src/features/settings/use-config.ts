/** The daemon config the settings page reads (`config.list`) and writes. */
import { useMutation, useQuery } from '@tanstack/react-query';
import type { UpdateChannel } from '@/api/types/update';

import { configMutations, configQueries } from '@/queries/config';

export interface ModpackConfig {
  'default-excludes'?: boolean;
  'exclude-files'?: string;
  'force-include-files'?: string;
  'overrides-exclusions'?: string;
}

interface ConfigEntries {
  home?: string;
  autostart?: boolean;
  defaults?: { memory?: string; 'jvm-args'?: string };
  content?: { 'curseforge-key'?: string };
  discord?: { enabled?: boolean };
  instance?: { 'multi-session'?: boolean };
  modpack?: ModpackConfig;
  update?: { channel?: UpdateChannel };
}

/**
 * The config read plus the two ways to write it. A toggle reports itself — the
 * control moves — so only a typed value is saved on purpose, and its button
 * follows the returned write.
 */
export function useConfig() {
  const config = useQuery(configQueries.list());
  const setConfig = useMutation(configMutations.set());

  return {
    entries: (config.data ?? {}) as ConfigEntries,
    pending: config.isPending,
    busy: setConfig.isPending,
    commit: (key: string, value: unknown) => setConfig.mutate({ key, value }),
    save: (key: string, value: unknown) =>
      setConfig.mutateAsync({ key, value }),
  };
}
