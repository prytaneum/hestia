import { FunnelSimpleIcon } from '@phosphor-icons/react';
import { Link, type LinkProps } from '@tanstack/react-router';
import { Fragment, type ReactNode } from 'react';

import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

/** One dimension of a filter: a labelled group of mutually exclusive options. */
export type FilterGroup = {
  label: string;
  value: string;
  options: {
    value: string;
    label: ReactNode;
    className?: string;
    disabled?: boolean;
    /** An entry that cannot be chosen but points somewhere, rather than a radio. */
    link?: LinkProps;
  }[];
  /** The value that reads as unfiltered; without one the group never tints. */
  neutral?: string;
  onChange: (value: string) => void;
};

/**
 * The filter control every filterable list shares: a funnel button, beside the
 * search field, opening one radio group per dimension. It tints while anything
 * is narrowed, so a list that is not showing everything says so with the menu
 * closed. A dimension with nothing to choose between passes `undefined` rather
 * than an empty group, and the button is gone once every dimension has.
 */
export function FilterMenu({
  groups,
  label = m['app.collection.filter'](),
  className,
}: {
  groups: (FilterGroup | undefined)[];
  label?: string;
  className?: string;
}) {
  const shown = groups.filter((g) => g !== undefined);
  if (shown.length === 0) return null;
  const filtered = shown.some(
    (g) => g.neutral !== undefined && g.value !== g.neutral,
  );
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="icon"
            aria-label={label}
            title={label}
            className={cn(
              filtered ? 'text-ember' : 'text-muted-foreground',
              className,
            )}
          >
            <FunnelSimpleIcon weight={filtered ? 'bold' : 'regular'} />
          </Button>
        }
      />
      <DropdownMenuContent align="end" className="w-44">
        {shown.map((group, i) => (
          <Fragment key={group.label}>
            {i > 0 && <DropdownMenuSeparator />}
            <DropdownMenuGroup>
              <DropdownMenuLabel>{group.label}</DropdownMenuLabel>
              <DropdownMenuRadioGroup
                value={group.value}
                onValueChange={(value) => group.onChange(String(value))}
              >
                {group.options
                  .filter((option) => !option.link)
                  .map((option) => (
                    <DropdownMenuRadioItem
                      key={option.value}
                      value={option.value}
                      disabled={option.disabled}
                      className={option.className}
                    >
                      {option.label}
                    </DropdownMenuRadioItem>
                  ))}
              </DropdownMenuRadioGroup>
              {group.options
                .filter((option) => option.link)
                .map((option) => (
                  <DropdownMenuItem
                    key={option.value}
                    render={<Link {...(option.link as LinkProps)} />}
                    className={option.className}
                  >
                    {option.label}
                  </DropdownMenuItem>
                ))}
            </DropdownMenuGroup>
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
