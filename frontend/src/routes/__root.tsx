import { TanStackDevtools } from '@tanstack/react-devtools';
import type { QueryClient } from '@tanstack/react-query';
import { ReactQueryDevtoolsPanel } from '@tanstack/react-query-devtools';
import { createRootRouteWithContext, Outlet } from '@tanstack/react-router';
import { TanStackRouterDevtoolsPanel } from '@tanstack/react-router-devtools';

import { Toaster } from '@/components/ui/sonner';

export interface RouterContext {
  queryClient: QueryClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: () => (
    <>
      <Outlet />
      <Toaster />
      {import.meta.env.DEV && (
        <TanStackDevtools
          plugins={[
            { name: 'Router', render: <TanStackRouterDevtoolsPanel /> },
            { name: 'Query', render: <ReactQueryDevtoolsPanel /> },
          ]}
        />
      )}
    </>
  ),
});
