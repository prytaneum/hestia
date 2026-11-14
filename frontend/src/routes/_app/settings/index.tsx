import { createFileRoute } from '@tanstack/react-router';

import { SETTINGS_TABS, type SettingsTab } from '@/features/settings/lib';
import { SettingsPage } from '@/features/settings/page';

export const Route = createFileRoute('/_app/settings/')({
  validateSearch: (
    search: Record<string, unknown>,
  ): { tab?: SettingsTab; plugin?: string } => ({
    tab: SETTINGS_TABS.includes(search.tab as SettingsTab)
      ? (search.tab as SettingsTab)
      : undefined,
    plugin:
      typeof search.plugin === 'string' && search.plugin
        ? search.plugin
        : undefined,
  }),
  component: Settings,
});

function Settings() {
  const { tab = 'general', plugin } = Route.useSearch();
  const navigate = Route.useNavigate();

  return (
    <SettingsPage
      tab={tab}
      plugin={plugin}
      onTabChange={(next) =>
        navigate({ search: next === 'general' ? {} : { tab: next } })
      }
    />
  );
}
