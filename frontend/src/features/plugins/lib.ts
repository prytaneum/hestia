import type { PluginPage, Setting } from '@/api';

export type PluginTab = `plugin:${string}`;

export const pluginTab = ({ plugin, page }: PluginPage): PluginTab =>
  `plugin:${plugin}/${page.id}`;

export const isPluginTab = (value: unknown): value is PluginTab =>
  typeof value === 'string' && value.startsWith('plugin:');

/** Mirrors the daemon's topic globs: an exact topic, or `prefix.*` for every topic beneath it. */
export function topicMatches(pattern: string, topic: string): boolean {
  if (!pattern.endsWith('*')) return pattern === topic;
  const prefix = pattern.slice(0, -1);
  return prefix.endsWith('.') && topic.startsWith(prefix);
}

export type SettingProblem =
  | { kind: 'min'; min: number }
  | { kind: 'max'; max: number }
  | { kind: 'pattern' };

/**
 * What is wrong with `text` as a value of `setting`, judged by the constraints
 * the manifest declares. The daemon checks the same rules; this only says so
 * before the write.
 */
export function settingProblem(
  setting: Setting,
  text: string,
): SettingProblem | null {
  if (!text) return null;
  if (setting.kind === 'number') {
    const value = Number(text);
    if (setting.min != null && value < setting.min)
      return { kind: 'min', min: setting.min };
    if (setting.max != null && value > setting.max)
      return { kind: 'max', max: setting.max };
    return null;
  }
  if (setting.pattern != null) {
    try {
      if (!new RegExp(`^(?:${setting.pattern})$`).test(text))
        return { kind: 'pattern' };
    } catch {
      return null;
    }
  }
  return null;
}

/** The setting's rows by group, in first-seen order; ungrouped settings come first. */
export function groupSettings(settings: Setting[]): [string, Setting[]][] {
  const groups = new Map<string, Setting[]>([['', []]]);
  for (const setting of settings) {
    const rows = groups.get(setting.group) ?? [];
    rows.push(setting);
    groups.set(setting.group, rows);
  }
  return [...groups].filter(([, rows]) => rows.length > 0);
}

export const isIconPath = (icon: string) => /[/.]/.test(icon);
