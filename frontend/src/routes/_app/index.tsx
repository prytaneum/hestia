import { createFileRoute } from '@tanstack/react-router';
import { LibraryPage } from '@/features/library/page';
import type { View } from '@/features/shared/entry/components';
import { flavorSearch, viewSearch } from '@/lib/entry-list-search';

type LibrarySearch = { view?: View; servers?: string; instances?: string };

export const Route = createFileRoute('/_app/')({
  validateSearch: (search: Record<string, unknown>): LibrarySearch => ({
    view: viewSearch(search.view),
    servers: flavorSearch(search.servers),
    instances: flavorSearch(search.instances),
  }),
  component: RouteComponent,
});

function RouteComponent() {
  const {
    view = 'grid',
    servers = 'all',
    instances = 'all',
  } = Route.useSearch();
  const navigate = Route.useNavigate();

  const patch = (next: LibrarySearch) =>
    navigate({ search: (prev) => ({ ...prev, ...next }), replace: true });

  return (
    <LibraryPage
      view={view}
      serverFlavor={servers}
      instanceFlavor={instances}
      onViewChange={(next) =>
        patch({ view: next === 'grid' ? undefined : next })
      }
      onServerFlavorChange={(next) =>
        patch({ servers: next === 'all' ? undefined : next })
      }
      onInstanceFlavorChange={(next) =>
        patch({ instances: next === 'all' ? undefined : next })
      }
    />
  );
}
