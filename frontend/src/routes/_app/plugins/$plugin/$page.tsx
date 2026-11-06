import { createFileRoute } from '@tanstack/react-router';

import { PluginPage } from '@/features/plugins';

export const Route = createFileRoute('/_app/plugins/$plugin/$page')({
  component: RouteComponent,
});

function RouteComponent() {
  const { plugin, page } = Route.useParams();
  return <PluginPage plugin={plugin} page={page} />;
}
