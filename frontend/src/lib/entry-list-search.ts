import type { View } from '@/features/shared/entry/components';

export type EntryListSearch = { view?: View; flavor?: string };

export function viewSearch(value: unknown): View | undefined {
  return value === 'list' ? 'list' : undefined;
}

export function flavorSearch(value: unknown): string | undefined {
  return typeof value === 'string' && value !== 'all' ? value : undefined;
}

export function validateEntryListSearch(
  search: Record<string, unknown>,
): EntryListSearch {
  return { view: viewSearch(search.view), flavor: flavorSearch(search.flavor) };
}

export function entryListProps(
  search: EntryListSearch,
  patch: (next: EntryListSearch) => void,
) {
  return {
    view: search.view ?? 'grid',
    flavor: search.flavor ?? 'all',
    onViewChange: (next: View) =>
      patch({ view: next === 'grid' ? undefined : next }),
    onFlavorChange: (next: string) =>
      patch({ flavor: next === 'all' ? undefined : next }),
  };
}
