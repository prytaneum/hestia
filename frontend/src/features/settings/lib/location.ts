import type { RequiredSetting } from '@/api';
import type { SettingsTab } from './search';

/**
 * Where the settings page edits a setting a content source waits on: the
 * Plugins tab with that plugin's row open, or the tab holding a launcher key.
 */
export function settingLocation({ plugin, key }: RequiredSetting): {
  tab: SettingsTab;
  plugin?: string;
} {
  if (plugin) return { tab: 'plugins', plugin };
  return { tab: key.startsWith('content.') ? 'content' : 'general' };
}
