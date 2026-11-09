import { useQuery } from '@tanstack/react-query';
import type { Edition, EntryKind } from '@/api';
import { catalogueQueries } from '@/queries/catalogue';

/** A flavor's display name from the daemon's catalogue, or its id until that loads. */
export function useFlavorName(
  kind: EntryKind,
  edition: Edition,
  id: string,
): string {
  const { data } = useQuery(catalogueQueries.flavors(kind));
  const catalogue = data?.editions.find((c) => c.edition === edition);
  return catalogue?.flavors.find((flavor) => flavor.id === id)?.name ?? id;
}
