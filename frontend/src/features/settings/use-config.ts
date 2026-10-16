/** The daemon config the settings page reads (`config.list`) and writes. */
import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { UpdateChannel } from '@/api/types/update';

import { m } from '@/paraglide/messages.js';
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
 * control moves — so only a typed value confirms with a toast, where nothing
 * else would tell you the daemon took it.
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
      setConfig.mutate(
        { key, value },
        { onSuccess: () => toast.success(m['app.toast.saved']()) },
      ),
  };
}
