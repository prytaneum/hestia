import type { Icon } from '@phosphor-icons/react';
import type { ReactNode } from 'react';

import { Card } from '@/components/ui/card';
import { Thumbnail } from '@/components/ui/thumbnail';
import { m } from '@/paraglide/messages.js';

/**
 * One listed thing in a browse grid: an icon tile the height of the card, a
 * name and author, one line of description, then a meta line that ends in the
 * card's one action. The height is fixed, so a grid of them stays even;
 * `problem` takes the description's line rather than adding one.
 *
 * Wrap it in a `group` element (usually the link it opens) for the hover tint.
 */
export function ListingCard({
  icon,
  image,
  title,
  author,
  badges,
  description,
  problem,
  meta,
  action,
}: {
  icon: Icon;
  image?: string;
  title: string;
  author?: string;
  badges?: ReactNode;
  description?: string;
  problem?: string;
  meta?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <Card size="sm" className="h-24 transition-colors group-hover:bg-muted/40">
      <div className="flex min-h-0 flex-1 gap-3 px-3">
        <Thumbnail
          src={image}
          icon={icon}
          size="2xl"
          className="aspect-square size-auto self-stretch"
        />
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-medium">{title}</span>
            {author && (
              <span className="shrink-0 text-[11px] text-muted-foreground">
                {m['content.browse.by_author']({ name: author })}
              </span>
            )}
            <span className="ml-auto flex shrink-0 items-center gap-1.5">
              {badges}
            </span>
          </div>
          {problem ? (
            <p
              className="mt-1 truncate text-xs text-destructive"
              title={problem}
            >
              {problem}
            </p>
          ) : (
            <p className="mt-1 truncate text-xs text-muted-foreground">
              {description}
            </p>
          )}
          <div className="mt-auto flex min-h-6 items-center gap-3 text-[11px] text-muted-foreground">
            {meta}
            <span className="ml-auto flex shrink-0 items-center gap-2">
              {action}
            </span>
          </div>
        </div>
      </div>
    </Card>
  );
}
