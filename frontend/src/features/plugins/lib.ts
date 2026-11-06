import type { PluginPage } from '@/api';

export type PluginTab = `plugin:${string}`;

export const pluginTab = ({ plugin, page }: PluginPage): PluginTab =>
  `plugin:${plugin}/${page.id}`;

export const isPluginTab = (value: unknown): value is PluginTab =>
  typeof value === 'string' && value.startsWith('plugin:');

export const isIconPath = (icon: string) => /[/.]/.test(icon);
