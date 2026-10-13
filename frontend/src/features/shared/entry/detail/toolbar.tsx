import type { ComponentProps } from 'react';

import { cn } from '@/lib/utils';

/**
 * The row of controls a detail tab opens with. Its height and the gap beneath
 * it are fixed here so every tab's body starts on the same line, whatever
 * controls the row holds.
 */
export function TabToolbar({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn('mb-3 flex min-h-8 items-center gap-2', className)}
      {...props}
    />
  );
}
