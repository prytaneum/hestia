/**
 * The settings index — every field's tab, group and searchable text.
 *
 * Search spans the whole page while only one tab is mounted, so a tab has to
 * report whether it holds a match without rendering: the text lives here rather
 * than being read off the fields. A new setting is one entry plus a `<Setting>`
 * wrapper at its call site.
 */
import { m } from '@/paraglide/messages.js';

export const SETTINGS_TABS = [
  'general',
  'java',
  'content',
  'instances',
  'storage',
] as const;

export type SettingsTab = (typeof SETTINGS_TABS)[number];

export type SettingsGroup =
  | 'general'
  | 'updates'
  | 'java-defaults'
  | 'java-runtimes'
  | 'content-sources'
  | 'modpack'
  | 'instance-behaviour'
  | 'sync'
  | 'storage'
  | 'daemon';

interface Entry {
  id: string;
  tab: SettingsTab;
  group: SettingsGroup;
  /** What the field says on screen, resolved in the active locale. */
  text: () => string;
}

const text =
  (...parts: Array<() => string>) =>
  () =>
    parts.map((part) => part()).join(' ');

const INDEX: Entry[] = [
  {
    id: 'language',
    tab: 'general',
    group: 'general',
    text: m['settings.language'],
  },
  {
    id: 'data-dir',
    tab: 'general',
    group: 'general',
    text: text(m['settings.data_dir'], m['settings.data_dir_hint']),
  },
  {
    id: 'autostart',
    tab: 'general',
    group: 'general',
    text: m['settings.start_at_login'],
  },
  {
    id: 'keep-open',
    tab: 'general',
    group: 'general',
    text: m['settings.keep_open'],
  },
  {
    id: 'close-action',
    tab: 'general',
    group: 'general',
    text: text(
      m['settings.close_action.label'],
      m['settings.close_action.hint'],
      m['settings.close_action.quit'],
      m['settings.close_action.tray'],
      m['settings.close_action.stop_daemon'],
    ),
  },
  {
    id: 'discord',
    tab: 'general',
    group: 'general',
    text: text(
      m['settings.discord.enabled_label'],
      m['settings.discord.enabled_description'],
    ),
  },
  {
    id: 'tutorial',
    tab: 'general',
    group: 'general',
    text: text(
      m['settings.tutorial.label'],
      m['settings.tutorial.hint'],
      m['settings.tutorial.replay'],
    ),
  },
  {
    id: 'update',
    tab: 'general',
    group: 'updates',
    text: text(m['settings.update.title'], m['settings.update.check']),
  },
  {
    id: 'default-memory',
    tab: 'java',
    group: 'java-defaults',
    text: text(m['settings.default_memory'], m['settings.default_memory_hint']),
  },
  {
    id: 'jvm-args',
    tab: 'java',
    group: 'java-defaults',
    text: m['settings.default_jvm_args'],
  },
  {
    id: 'runtimes',
    tab: 'java',
    group: 'java-runtimes',
    text: text(
      m['settings.java.runtimes'],
      m['settings.java.runtimes_hint'],
      m['settings.java.not_installed'],
    ),
  },
  {
    id: 'curseforge-key',
    tab: 'content',
    group: 'content-sources',
    text: text(m['settings.curseforge_key'], m['settings.curseforge_key_hint']),
  },
  {
    id: 'modpack',
    tab: 'content',
    group: 'modpack',
    text: text(
      m['settings.modpack.title'],
      m['settings.modpack.hint'],
      m['settings.modpack.default_excludes'],
      m['settings.modpack.force_include'],
      m['settings.modpack.exclude'],
      m['settings.modpack.overrides_exclusions'],
    ),
  },
  {
    id: 'multi-session',
    tab: 'instances',
    group: 'instance-behaviour',
    text: text(
      m['settings.instances.multi_session_label'],
      m['settings.instances.multi_session_description'],
    ),
  },
  {
    id: 'sync-units',
    tab: 'instances',
    group: 'sync',
    text: text(
      m['settings.sync.section'],
      m['settings.sync.description'],
      m['domain.sync_unit.options.label'],
      m['domain.sync_unit.servers.label'],
      m['domain.sync_unit.commands.label'],
      m['domain.sync_unit.hotbars.label'],
    ),
  },
  {
    id: 'sync-options',
    tab: 'instances',
    group: 'sync',
    text: text(
      m['settings.sync.options.label'],
      m['settings.sync.options.hint'],
    ),
  },
  {
    id: 'cache',
    tab: 'storage',
    group: 'storage',
    text: text(m['settings.download_cache'], m['settings.cache.clear']),
  },
  {
    id: 'daemon',
    tab: 'storage',
    group: 'daemon',
    text: text(m['settings.daemon.title'], m['settings.daemon.hint']),
  },
];

export interface SettingsMatch {
  ids: Set<string>;
  groups: Set<SettingsGroup>;
  perTab: Record<SettingsTab, number>;
  total: number;
}

/**
 * Every setting whose text carries all of the query's words. An empty query
 * matches everything, which is what an unfiltered page renders.
 */
export function search(query: string): SettingsMatch {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hits = terms.length
    ? INDEX.filter((entry) => {
        const haystack = entry.text().toLowerCase();
        return terms.every((term) => haystack.includes(term));
      })
    : INDEX;

  const perTab = Object.fromEntries(
    SETTINGS_TABS.map((tab) => [tab, 0]),
  ) as Record<SettingsTab, number>;
  for (const entry of hits) perTab[entry.tab] += 1;

  return {
    ids: new Set(hits.map((entry) => entry.id)),
    groups: new Set(hits.map((entry) => entry.group)),
    perTab,
    total: hits.length,
  };
}

/** The first tab holding a match, for the jump a filtered page performs. */
export function firstTabWithMatch(match: SettingsMatch): SettingsTab | null {
  return SETTINGS_TABS.find((tab) => match.perTab[tab] > 0) ?? null;
}
