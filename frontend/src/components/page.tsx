import type { ReactNode } from 'react';

import { useSearch } from '@/components/app-shell/search-context';
import { SearchInput } from '@/components/search-input';
import { anchor } from '@/features/onboarding';
import { m } from '@/paraglide/messages.js';

/**
 * A full-bleed page: header with title/actions over a scrolling body. Every
 * routed page uses this so the chrome stays consistent app-wide. While
 * `loading`, the body swaps for `skeleton` and the header stays live.
 */
export function Page({
  title,
  subtitle,
  search,
  searchPlaceholder,
  actions,
  skeleton,
  loading = false,
  children,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  search?: boolean;
  searchPlaceholder?: string;
  actions?: ReactNode;
  skeleton?: ReactNode;
  loading?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b border-border px-5 py-3">
        <div className="min-w-0">
          <h1 className="truncate font-heading text-base font-semibold">
            {title}
          </h1>
          {subtitle && (
            <p className="truncate text-xs text-muted-foreground">{subtitle}</p>
          )}
        </div>
        <div className="ml-auto flex items-center gap-2">
          {search && (
            <span {...anchor('page-search')}>
              <PageSearch
                placeholder={searchPlaceholder ?? m['app.search.placeholder']()}
              />
            </span>
          )}
          {actions && (
            <span
              className="flex items-center gap-2"
              {...anchor('page-actions')}
            >
              {actions}
            </span>
          )}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {loading && skeleton ? skeleton : children}
      </div>
    </div>
  );
}

function PageSearch({ placeholder }: { placeholder: string }) {
  const { query, setQuery } = useSearch();
  return (
    <SearchInput
      value={query}
      onChange={setQuery}
      placeholder={placeholder}
      className="w-56"
    />
  );
}

/** A labelled group within a page body. */
export function Section({
  title,
  count,
  action,
  className,
  children,
}: {
  title: string;
  count?: number;
  action?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={className}>
      <div className="mb-3 flex items-center gap-2">
        <h2 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
          {title}
        </h2>
        {count != null && (
          <span className="font-mono text-[11px] text-muted-foreground">
            {count}
          </span>
        )}
        {action && <div className="ml-auto">{action}</div>}
      </div>
      {children}
    </section>
  );
}

/** A small muted count beside a tab label ("Content 5"). */
export function TabCount({ n }: { n: number }) {
  return (
    <span className="ml-1.5 font-mono text-[10px] text-muted-foreground">
      {n}
    </span>
  );
}

/** A key/value row for detail/overview panels. */
export function Stat({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-1.5">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="text-right font-mono text-xs">{value}</span>
    </div>
  );
}
