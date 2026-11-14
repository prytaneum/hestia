import type { RequiredSetting } from '@/api';
import type { SettingsTab } from './search';

/** The element id of a plugin setting's control. */
export const pluginFieldId = (plugin: string, key: string) =>
  `${plugin}-${key}`;

/**
 * Where the settings page edits a setting a content source waits on: the
 * Plugins tab with that plugin's row open, or the tab holding a launcher key.
 * `field` is the element id of its control, which the page scrolls to.
 */
export function settingLocation({ plugin, key }: RequiredSetting): {
  tab: SettingsTab;
  plugin?: string;
  field: string;
} {
  if (plugin) {
    return { tab: 'plugins', plugin, field: pluginFieldId(plugin, key) };
  }
  return {
    tab: key.startsWith('content.') ? 'content' : 'general',
    field: key.slice(key.indexOf('.') + 1),
  };
}
