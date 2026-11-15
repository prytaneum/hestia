import type { RequiredSetting } from '@/api';

/** The element id of a plugin setting's control. */
export const pluginFieldId = (plugin: string, key: string) =>
  `${plugin}-${key}`;

/**
 * Where a setting a content source waits on is edited: that plugin's page.
 * `field` is the element id of its control, which the page scrolls to.
 */
export function settingLocation({ plugin, key }: RequiredSetting) {
  return {
    to: '/plugins/$plugin',
    params: { plugin },
    search: { field: pluginFieldId(plugin, key) },
  } as const;
}
