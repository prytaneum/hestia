import type { ContentKind } from '@/api';
import { kindInfo } from '@/features/shared/content/lib';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

const chipClass = (active: boolean) =>
  cn(
    'border px-2.5 py-1 text-xs transition-colors outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-40',
    active
      ? 'border-transparent bg-primary text-primary-foreground'
      : 'border-border text-muted-foreground hover:bg-muted hover:text-foreground',
  );

/**
 * The content-type dimension shared by every content-shaped list (browse, an
 * entry's content tab, the install picker), as a row of chips so every kind a
 * list can show is on screen. `count` adds the tally each list already knows;
 * `all` is dropped where a kind is always one of them, as in the install
 * picker.
 */
export function KindChips({
  kinds,
  kind,
  onKindChange,
  count,
  disabled,
  all = true,
  className,
}: {
  kinds: ContentKind[];
  kind?: ContentKind;
  onKindChange: (kind?: ContentKind) => void;
  count?: (kind: ContentKind) => number;
  disabled?: (kind: ContentKind) => boolean;
  all?: boolean;
  className?: string;
}) {
  return (
    <div className={cn('flex flex-wrap items-center gap-1.5', className)}>
      {all && (
        <button
          type="button"
          aria-pressed={!kind}
          className={chipClass(!kind)}
          onClick={() => onKindChange(undefined)}
        >
          {m['app.label.all']()}
        </button>
      )}
      {kinds.map((k) => (
        <button
          key={k}
          type="button"
          aria-pressed={kind === k}
          disabled={disabled?.(k)}
          className={chipClass(kind === k)}
          onClick={() => onKindChange(k)}
        >
          {kindInfo[k].label()}
          {count && (
            <span className="ml-1.5 font-mono text-[10px] opacity-60">
              {count(k)}
            </span>
          )}
        </button>
      ))}
    </div>
  );
}
