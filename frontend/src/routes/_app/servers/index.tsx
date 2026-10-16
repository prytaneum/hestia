import { createFileRoute } from '@tanstack/react-router';
import { ServersPage } from '@/features/servers/page';
import {
  entryListProps,
  validateEntryListSearch,
} from '@/lib/entry-list-search';

export const Route = createFileRoute('/_app/servers/')({
  validateSearch: validateEntryListSearch,
  component: RouteComponent,
});

function RouteComponent() {
  const search = Route.useSearch();
  const navigate = Route.useNavigate();

  return (
    <ServersPage
      {...entryListProps(search, (next) =>
        navigate({ search: (prev) => ({ ...prev, ...next }), replace: true }),
      )}
    />
  );
}
