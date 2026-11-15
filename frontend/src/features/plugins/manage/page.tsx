import { useQuery } from '@tanstack/react-query';

import { useSearch } from '@/components/app-shell/search-context';
import { chipClass } from '@/components/chip';

import { Page } from '@/components/page';
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
 * the repositories themselves, as chips over one search, the way the content
 * browse page narrows by kind. One install flow serves every view, so a review
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
  const { query } = useSearch();
  const labels: Record<PluginsTab, string> = {
    installed: m['settings.plugins.tabs.installed'](),
    browse: m['settings.plugins.tabs.browse'](),
    repositories: m['settings.plugins.repos.title'](),
  };

  return (
    <Page
      title={m['app.nav.plugins']()}
      subtitle={m['settings.plugins.hint']()}
      search={tab !== 'repositories'}
      searchPlaceholder={m['settings.plugins.browse.search']()}
      actions={<InstallMenu install={install} />}
    >
      <div className="mb-5 flex flex-wrap items-center gap-1.5">
        {PLUGINS_TABS.map((name) => (
          <button
            key={name}
            type="button"
            aria-pressed={tab === name}
            className={chipClass(tab === name)}
            onClick={() => onTabChange(name)}
          >
            {labels[name]}
            {name === 'installed' && updates > 0 && (
              <span
                className="ml-1.5 font-mono text-[10px]"
                title={m['settings.plugins.updates_available']({
                  count: updates,
                })}
              >
                {updates}
              </span>
            )}
          </button>
        ))}
      </div>

      <div className="space-y-4 pb-6">
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
        {tab === 'installed' && (
          <Installed
            query={query}
            install={install}
            onBrowse={() => onTabChange('browse')}
          />
        )}
        {tab === 'browse' && (
          <PluginBrowse
            query={query}
            install={install}
            onRepositories={() => onTabChange('repositories')}
          />
        )}
        {tab === 'repositories' && (
          <div className="max-w-3xl">
            <RepositoryList />
          </div>
        )}
      </div>
    </Page>
  );
}
