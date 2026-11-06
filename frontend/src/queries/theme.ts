/**
 * The active plugin theme: a desktop pref naming `{ plugin, id }`, resolved
 * against the themes of the plugins that are enabled right now. Nothing is
 * injected until the user has chosen one, and a theme whose plugin is gone,
 * disabled or failing falls back to the built-in look.
 */
import { useQuery } from '@tanstack/react-query';
import type { PluginTheme } from '../api';
import { safeMode } from '../api/prefs';
import { keys } from './keys';
import { pluginQueries } from './plugin';
import { usePrefs } from './prefs';

export const THEME_KEY = 'theme';

type ThemeChoice = { plugin: string; id: string };

function parseChoice(value: unknown): ThemeChoice | null {
  if (typeof value !== 'object' || value === null) return null;
  const { plugin, id } = value as Record<string, unknown>;
  return typeof plugin === 'string' && typeof id === 'string'
    ? { plugin, id }
    : null;
}

/** `undefined` until the shell has answered, so nothing is applied before it does. */
export function useSafeMode(): boolean | undefined {
  const { data } = useQuery({
    queryKey: keys.prefs.safeMode(),
    queryFn: safeMode,
    staleTime: Number.POSITIVE_INFINITY,
  });
  return data;
}

export function useTheme() {
  const prefs = usePrefs();
  const { data: themes = [] } = useQuery(pluginQueries.themes());
  const choice = parseChoice(prefs.get<unknown>(THEME_KEY, null));
  const active: PluginTheme | null =
    themes.find(
      (theme) => theme.plugin === choice?.plugin && theme.id === choice?.id,
    ) ?? null;

  return {
    themes,
    active,
    ready: prefs.ready,
    select: (theme: PluginTheme | null) =>
      theme
        ? prefs.set(THEME_KEY, { plugin: theme.plugin, id: theme.id })
        : prefs.remove(THEME_KEY),
  };
}
