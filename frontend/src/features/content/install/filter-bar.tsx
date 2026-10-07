import type { ReactNode } from 'react';

import { SearchInput } from '@/components/search-input';

/** Search field plus an optional filter menu, shared by the target/content steps. */
export function FilterBar({
  search,
  onSearch,
  placeholder,
  trailing,
}: {
  search: string;
  onSearch: (v: string) => void;
  placeholder: string;
  /** A filter control beside the search field (the source menu). */
  trailing?: ReactNode;
}) {
  return (
    <div className="mb-3 flex items-center gap-2">
      <SearchInput
        value={search}
        onChange={onSearch}
        placeholder={placeholder}
        className="min-w-0 flex-1"
      />
      {trailing}
    </div>
  );
}
