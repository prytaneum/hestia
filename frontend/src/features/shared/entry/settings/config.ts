import { toast } from 'sonner';

import type { ConfigEntry } from '@/api';
import { m } from '@/paraglide/messages.js';

export interface SetConfig {
  mutate: (entry: ConfigEntry, options?: { onSuccess?: () => void }) => void;
  mutateAsync: (entry: ConfigEntry) => Promise<unknown>;
  isPending: boolean;
}

export function configValue(
  config: ConfigEntry[] | undefined,
  key: string,
): string {
  return config?.find((e) => e.key === key)?.value ?? '';
}

/**
 * The two ways an entry setting is written, as the launcher's own are: a
 * control that moves reports itself, so only a typed value confirms with a
 * toast.
 */
export function entryConfig(setConfig: SetConfig) {
  return {
    commit: (key: string, value: string) => setConfig.mutate({ key, value }),
    save: (key: string, value: string) =>
      setConfig.mutate(
        { key, value },
        { onSuccess: () => toast.success(m['app.toast.saved']()) },
      ),
  };
}
