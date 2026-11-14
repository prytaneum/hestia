import { useQueries } from '@tanstack/react-query';

import { getLocale } from '@/paraglide/runtime.js';
import { pluginQueries } from '@/queries/plugin';

/**
 * A plugin's own message for `key` in the app's language, or `fallback` (the
 * manifest's text) while the catalog loads, when it ships none, or when it
 * lacks the key. Keys by convention: `page.<id>.title`, `setting.<key>.label`,
 * `setting.<key>.help`, `setting.<key>.placeholder`.
 */
export type PluginText = (
  plugin: string,
  key: string,
  fallback: string,
) => string;

/** The catalogs of `plugins`, one request each however many texts are read. */
export function usePluginText(plugins: string[]): PluginText {
  const locale = getLocale();
  const unique = [...new Set(plugins)];
  const catalogs = useQueries({
    queries: unique.map((plugin) => pluginQueries.messages(plugin, locale)),
  });
  const byPlugin = new Map(
    unique.map((plugin, i) => [plugin, catalogs[i]?.data]),
  );
  return (plugin, key, fallback) => byPlugin.get(plugin)?.[key] ?? fallback;
}
