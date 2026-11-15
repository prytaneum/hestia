import { useQuery } from '@tanstack/react-query';

import { Page, TabCount } from '@/components/page';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Notice } from '@/components/warning-notice';
import { m } from '@/paraglide/messages.js';
import { useDaemon } from '@/queries/daemon';
import { pluginQueries } from '@/queries/plugin';

import { PluginBrowse } from './browse';
import { InstallMenu, InstallReview, usePluginInstall } from './install';
import { Installed } from './installed';
import { RepositoryList } from './repositories';

export const PLUGINS_TABS = ['installed', 'browse', 'repositories'] as const;

export type PluginsTab = (typeof PLUGINS_TABS)[number];

/**
 * Plugin management: what is installed, what the added repositories offer, and
 * the repositories themselves. One install flow serves every tab, so a review
 * it raises shows wherever the user is.
 */
export function PluginsPage({
  tab,
  onTabChange,
}: {
  tab: PluginsTab;
  onTabChange: (tab: PluginsTab) => void;
}) {
  const install = usePluginInstall();
  const outdated = useQuery(pluginQueries.outdated());
  const daemon = useDaemon();
  const updates = outdated.data?.plugins.length ?? 0;

  return (
    <Page
      title={m['app.nav.plugins']()}
      subtitle={m['settings.plugins.hint']()}
      actions={<InstallMenu install={install} />}
    >
      <Tabs
        value={tab}
        onValueChange={(value) => onTabChange(value as PluginsTab)}
        className="gap-0"
      >
        <TabsList
          variant="line"
          className="h-auto gap-6 border-b border-border"
        >
          <TabsTrigger value="installed">
            {m['settings.plugins.tabs.installed']()}
            {updates > 0 && (
              <span
                title={m['settings.plugins.updates_available']({
                  count: updates,
                })}
              >
                <TabCount n={updates} />
              </span>
            )}
          </TabsTrigger>
          <TabsTrigger value="browse">
            {m['settings.plugins.tabs.browse']()}
          </TabsTrigger>
          <TabsTrigger value="repositories">
            {m['settings.plugins.repos.title']()}
          </TabsTrigger>
        </TabsList>

        <div className="max-w-3xl space-y-4 pt-5 pb-6">
          {daemon.status?.safeMode && (
            <Notice message={m['app.daemon.safe_mode_hint']()} />
          )}
          {install.review && (
            <InstallReview
              inspection={install.review.inspection}
              onContinue={() => {
                if (install.review) void install.approve(install.review);
              }}
              onCancel={install.dismiss}
            />
          )}
          <TabsContent value="installed">
            <Installed
              install={install}
              onBrowse={() => onTabChange('browse')}
            />
          </TabsContent>
          <TabsContent value="browse">
            <PluginBrowse
              install={install}
              onRepositories={() => onTabChange('repositories')}
            />
          </TabsContent>
          <TabsContent value="repositories">
            <RepositoryList />
          </TabsContent>
        </div>
      </Tabs>
    </Page>
  );
}
