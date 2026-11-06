import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';

import type { EntryKind, PageSlot, PluginPage } from '@/api';
import type { NavItem } from '@/components/app-shell/nav';
import { SettingGroup } from '@/components/setting-controls';
import { TabsContent, TabsTrigger } from '@/components/ui/tabs';
import { pluginQueries } from '@/queries/plugin';

import { PluginIcon } from './icon';
import { pluginTab } from './lib';
import { PluginView } from './view';

function usePluginPages(slot: PageSlot, kind?: EntryKind): PluginPage[] {
  const { data } = useQuery(pluginQueries.pages());
  return useMemo(
    () =>
      (data ?? []).filter(
        ({ page }) => page.slot === slot && (!kind || page.kind === kind),
      ),
    [data, slot, kind],
  );
}

export function pagePath(plugin: string, page: string): string {
  return `/plugins/${encodeURIComponent(plugin)}/${encodeURIComponent(page)}`;
}

/** The nav slot: one sidebar item per contributed page, after the built-in ones. */
export function usePluginNav(): NavItem[] {
  const pages = usePluginPages('nav');
  return useMemo(
    () =>
      pages.map(({ plugin, page }) => {
        const to = pagePath(plugin, page.id);
        return {
          to,
          match: [to],
          label: () => page.title,
          icon: (props) => (
            <PluginIcon plugin={plugin} icon={page.icon} {...props} />
          ),
        };
      }),
    [pages],
  );
}

/** The entry slot's tab triggers, for a server or instance detail's tab list. */
export function PluginTabTriggers({ kind }: { kind: EntryKind }) {
  return usePluginPages('entry', kind).map((contribution) => (
    <TabsTrigger
      key={pluginTab(contribution)}
      value={pluginTab(contribution)}
    >
      {contribution.page.title}
    </TabsTrigger>
  ));
}

export function PluginTabContents({
  kind,
  entry,
}: {
  kind: EntryKind;
  entry: string;
}) {
  return usePluginPages('entry', kind).map((contribution) => (
    <TabsContent
      key={pluginTab(contribution)}
      value={pluginTab(contribution)}
      className="p-5"
    >
      <PluginView
        plugin={contribution.plugin}
        page={contribution.page.id}
        entry={entry}
      />
    </TabsContent>
  ));
}

/** The settings slot: a section per contributed page. */
export function PluginSettingsSections() {
  const pages = usePluginPages('settings');
  return pages.map(({ plugin, page }) => (
    <SettingGroup key={`${plugin}/${page.id}`} legend={page.title}>
      <PluginView plugin={plugin} page={page.id} />
    </SettingGroup>
  ));
}
