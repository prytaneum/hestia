import { WarningIcon } from '@phosphor-icons/react';
import type { ReactNode } from 'react';
import { type WarningInfo, warningHint, warningMessage } from '@/api';
import { cn } from '@/lib/utils';

export function Notice({
  message,
  hint,
}: {
  message: ReactNode;
  hint?: ReactNode;
}) {
  return (
    <div className="flex gap-2.5 border border-amber/40 bg-amber/5 px-3 py-2.5">
      <WarningIcon className="mt-0.5 size-4 shrink-0 text-amber" />
      <div className="space-y-1">
        <p className="text-xs leading-relaxed">{message}</p>
        {hint && (
          <p className="text-xs leading-relaxed text-muted-foreground">
            {hint}
          </p>
        )}
      </div>
    </div>
  );
}

/**
 * The standing form of a daemon warning: a degraded state that is still true,
 * shown where the thing it degrades lives (a server's overview, say) rather than
 * as a toast that scrolls past. Each warning renders its localized headline over
 * the hint for fixing it.
 */
export function WarningNotice({
  warnings,
  className,
}: {
  warnings: WarningInfo[] | undefined;
  className?: string;
}) {
  if (!warnings?.length) return null;
  return (
    <div className={cn('space-y-2', className)}>
      {warnings.map((warning) => (
        <Notice
          key={`${warning.kind}:${warningMessage(warning)}`}
          message={warningMessage(warning)}
          hint={warningHint(warning)}
        />
      ))}
    </div>
  );
}
