import { createFileRoute } from '@tanstack/react-router';

import { PluginDetailPage } from '@/features/plugins';

export const Route = createFileRoute('/_app/plugins/$plugin/')({
  validateSearch: (search: Record<string, unknown>): { field?: string } => ({
    field:
      typeof search.field === 'string' && search.field
        ? search.field
        : undefined,
  }),
  component: RouteComponent,
});

function RouteComponent() {
  const { plugin } = Route.useParams();
  return <PluginDetailPage id={plugin} />;
}
