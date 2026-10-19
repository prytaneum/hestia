import type { Icon } from '@phosphor-icons/react';
import { CaretRightIcon } from '@phosphor-icons/react';
import { Link, type LinkProps } from '@tanstack/react-router';
import type { ReactNode } from 'react';

import { Thumbnail } from '@/components/ui/thumbnail';

/** Breadcrumb + banner hero: parent link, big icon, name, badges, actions. */
export function DetailHero({
  parentLabel,
  parentTo,
  parentParams,
  parentSearch,
  icon: Icon,
  iconUrl,
  iconAction,
  name,
  badges,
  actions,
  busy,
}: {
  parentLabel: string;
  parentTo: LinkProps['to'];
  parentParams?: LinkProps['params'];
  parentSearch?: LinkProps['search'];
  icon: Icon;
  /** A remote icon (a content project's) shown in place of the glyph. */
  iconUrl?: string;
  /** An overlay control on the icon tile, revealed on hover. */
  iconAction?: ReactNode;
  name: string;
  badges: ReactNode;
  actions: ReactNode;
  /** A running job on the entry, shown under the banner. */
  busy?: ReactNode;
}) {
  return (
    <div className="border-b border-border">
      <div className="flex items-center gap-1.5 px-5 py-2 text-xs text-muted-foreground">
        <Link
          to={parentTo}
          params={parentParams}
          search={parentSearch}
          className="hover:text-foreground"
        >
          {parentLabel}
        </Link>
        <CaretRightIcon className="size-3" />
        <span className="text-foreground">{name}</span>
      </div>

      <div className="bg-muted/25">
        <div className="flex items-end gap-4 px-5 pt-8 pb-5">
          <Thumbnail src={iconUrl} icon={Icon} size="2xl" className="group">
            {iconAction && (
              <span className="absolute right-1 bottom-1 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100">
                {iconAction}
              </span>
            )}
          </Thumbnail>
          <div className="min-w-0 flex-1">
            <h1 className="truncate font-heading text-xl font-semibold">
              {name}
            </h1>
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              {badges}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">{actions}</div>
        </div>
        {busy && <div className="px-5 pb-4">{busy}</div>}
      </div>
    </div>
  );
}
