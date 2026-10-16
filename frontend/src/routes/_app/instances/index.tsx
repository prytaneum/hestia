import { createFileRoute } from '@tanstack/react-router';
import { InstancesPage } from '@/features/instances/page';
import {
  entryListProps,
  validateEntryListSearch,
} from '@/lib/entry-list-search';

export const Route = createFileRoute('/_app/instances/')({
  validateSearch: validateEntryListSearch,
  component: RouteComponent,
});

function RouteComponent() {
  const search = Route.useSearch();
  const navigate = Route.useNavigate();

  return (
    <InstancesPage
      {...entryListProps(search, (next) =>
        navigate({ search: (prev) => ({ ...prev, ...next }), replace: true }),
      )}
    />
  );
}
