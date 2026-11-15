import { createFileRoute } from '@tanstack/react-router';

import { SETTINGS_TABS, type SettingsTab } from '@/features/settings/lib';
import { SettingsPage } from '@/features/settings/page';

export const Route = createFileRoute('/_app/settings/')({
  validateSearch: (search: Record<string, unknown>): { tab?: SettingsTab } => ({
    tab: SETTINGS_TABS.includes(search.tab as SettingsTab)
      ? (search.tab as SettingsTab)
      : undefined,
  }),
  component: Settings,
});

function Settings() {
  const { tab = 'general' } = Route.useSearch();
  const navigate = Route.useNavigate();

  return (
    <SettingsPage
      tab={tab}
      onTabChange={(next) =>
        navigate({ search: next === 'general' ? {} : { tab: next } })
      }
    />
  );
}
