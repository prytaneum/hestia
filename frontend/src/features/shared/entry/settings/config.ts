import { toast } from 'sonner';

import type { ConfigEntry } from '@/api';
import { errorMessage } from '@/api';
import { m } from '@/paraglide/messages.js';

export interface SetConfig {
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
 * Write `entries` in order and report the outcome. The daemon takes one key per
 * call, so a rejected key stops the run and leaves the ones before it saved.
 */
export async function saveConfig(
  setConfig: SetConfig,
  entries: ConfigEntry[],
): Promise<boolean> {
  try {
    for (const entry of entries) {
      await setConfig.mutateAsync(entry);
    }
    toast.success(m['app.toast.saved']());
    return true;
  } catch (error) {
    toast.error(errorMessage(error));
    return false;
  }
}
