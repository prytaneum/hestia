import type { ConfigEntry } from '@/api';

export interface SetConfig {
  mutate: (entry: ConfigEntry) => void;
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
 * control that moves reports itself, so only a typed value is saved on
 * purpose, and its button follows the returned write.
 */
export function entryConfig(setConfig: SetConfig) {
  return {
    commit: (key: string, value: string) => setConfig.mutate({ key, value }),
    save: (key: string, value: string) => setConfig.mutateAsync({ key, value }),
  };
}
