import { useQuery } from '@tanstack/react-query';
import type { EntryKind } from '@/api';
import { catalogueQueries } from '@/queries/catalogue';

/** A flavor's display name from the daemon's catalogue, or its id until that loads. */
export function useFlavorName(kind: EntryKind, id: string): string {
  const { data } = useQuery(catalogueQueries.flavors(kind));
  return data?.flavors.find((flavor) => flavor.id === id)?.name ?? id;
}
