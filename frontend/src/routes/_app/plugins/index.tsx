import { createFileRoute } from '@tanstack/react-router';

import { PLUGINS_TABS, PluginsPage, type PluginsTab } from '@/features/plugins';

export const Route = createFileRoute('/_app/plugins/')({
  validateSearch: (search: Record<string, unknown>): { tab?: PluginsTab } => ({
    tab: PLUGINS_TABS.includes(search.tab as PluginsTab)
      ? (search.tab as PluginsTab)
      : undefined,
  }),
  component: Plugins,
});

function Plugins() {
  const { tab = 'installed' } = Route.useSearch();
  const navigate = Route.useNavigate();

  return (
    <PluginsPage
      tab={tab}
      onTabChange={(next) =>
        navigate({ search: next === 'installed' ? {} : { tab: next } })
      }
    />
  );
}
