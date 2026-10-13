import { useMemo, useState } from 'react';

import { Page, TabCount } from '@/components/page';
import { SearchInput } from '@/components/search-input';
import { Bone } from '@/components/skeleton';
import { FieldGroup } from '@/components/ui/field';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { SettingsFilter } from '@/features/settings/components';
import {
  firstTabWithMatch,
  SETTINGS_TABS,
  type SettingsTab,
  search,
} from '@/features/settings/lib';
import {
  ContentTab,
  GeneralTab,
  InstancesTab,
  JavaTab,
  StorageTab,
} from '@/features/settings/tabs';
import { useConfig } from '@/features/settings/use-config';
import { m } from '@/paraglide/messages.js';
import { usePrefs } from '@/queries/prefs';

const TAB_LABELS: Record<SettingsTab, () => string> = {
  general: m['settings.tabs.general'],
  java: m['settings.tabs.java'],
  content: m['settings.tabs.content'],
  instances: m['settings.tabs.instances'],
  storage: m['settings.tabs.storage'],
};

/**
 * Settings, one tab per area. Search spans every tab — a query narrows each
 * tab to its matching fields and moves to the tab holding them, so a setting
 * is found without knowing which area owns it.
 */
export function SettingsPage({
  tab,
  onTabChange,
}: {
  tab: SettingsTab;
  onTabChange: (tab: SettingsTab) => void;
}) {
  const { pending } = useConfig();
  const prefs = usePrefs();
  const [query, setQuery] = useState('');

  const match = useMemo(() => (query.trim() ? search(query) : null), [query]);
  const active =
    match && match.perTab[tab] === 0 ? (firstTabWithMatch(match) ?? tab) : tab;

  return (
    <Page
      title={m['app.nav.settings']()}
      subtitle={m['settings.subtitle']()}
      loading={pending || !prefs.ready}
      actions={
        <SearchInput
          value={query}
          onChange={setQuery}
          placeholder={m['settings.search_placeholder']()}
          className="w-56"
          delay={120}
        />
      }
      skeleton={
        <div className="max-w-2xl space-y-8">
          {[0, 1, 2].map((group) => (
            <div key={group} className="space-y-5">
              <Bone className="h-4 w-32" />
              {[0, 1].map((field) => (
                <div key={field} className="space-y-2">
                  <Bone className="h-3 w-24" />
                  <Bone className="h-9 max-w-md" />
                </div>
              ))}
            </div>
          ))}
        </div>
      }
    >
      <Tabs
        value={active}
        onValueChange={(value) => onTabChange(value as SettingsTab)}
        className="gap-0"
      >
        <TabsList
          variant="line"
          className="h-auto gap-6 border-b border-border"
        >
          {SETTINGS_TABS.map((name) => (
            <TabsTrigger key={name} value={name}>
              {TAB_LABELS[name]()}
              {match && <TabCount n={match.perTab[name]} />}
            </TabsTrigger>
          ))}
        </TabsList>

        <SettingsFilter match={match}>
          {match?.total === 0 ? (
            <p className="py-8 text-sm text-muted-foreground">
              {m['settings.search_empty']({ query })}
            </p>
          ) : (
            <div className="max-w-2xl pt-5 pb-6">
              <TabsContent value="general">
                <FieldGroup>
                  <GeneralTab />
                </FieldGroup>
              </TabsContent>
              <TabsContent value="java">
                <FieldGroup>
                  <JavaTab />
                </FieldGroup>
              </TabsContent>
              <TabsContent value="content">
                <FieldGroup>
                  <ContentTab />
                </FieldGroup>
              </TabsContent>
              <TabsContent value="instances">
                <FieldGroup>
                  <InstancesTab />
                </FieldGroup>
              </TabsContent>
              <TabsContent value="storage">
                <FieldGroup>
                  <StorageTab />
                </FieldGroup>
              </TabsContent>
            </div>
          )}
        </SettingsFilter>
      </Tabs>
    </Page>
  );
}
