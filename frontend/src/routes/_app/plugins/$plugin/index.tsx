import { createFileRoute } from '@tanstack/react-router';

import {
  PLUGIN_DETAIL_TABS,
  PluginDetailPage,
  type PluginDetailTab,
} from '@/features/plugins';

export const Route = createFileRoute('/_app/plugins/$plugin/')({
  validateSearch: (
    search: Record<string, unknown>,
  ): { tab?: PluginDetailTab; field?: string } => ({
    tab: PLUGIN_DETAIL_TABS.includes(search.tab as PluginDetailTab)
      ? (search.tab as PluginDetailTab)
      : undefined,
    field:
      typeof search.field === 'string' && search.field
        ? search.field
        : undefined,
  }),
  component: RouteComponent,
});

function RouteComponent() {
  const { plugin } = Route.useParams();
  const { tab, field } = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <PluginDetailPage
      id={plugin}
      tab={tab ?? (field ? 'settings' : 'details')}
      onTabChange={(next) =>
        navigate({ search: next === 'details' ? {} : { tab: next } })
      }
    />
  );
}
