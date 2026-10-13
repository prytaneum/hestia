import type { ContentVersion } from '@/api';
import { Badge } from '@/components/ui/badge';
import { agoLabel } from '@/lib/format';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

export function VersionOption({
  version,
  selected,
  current,
  onSelect,
}: {
  version: ContentVersion;
  selected: boolean;
  current: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      className={cn(
        'flex w-full items-center gap-2.5 px-3 py-2 text-left outline-none transition-colors focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset',
        selected ? 'bg-muted text-foreground' : 'hover:bg-muted/50',
      )}
    >
      <span
        className={cn(
          'size-1.5 shrink-0 rounded-full',
          selected ? 'bg-ember' : 'bg-transparent',
        )}
      />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-mono text-xs">
          {version.versionNumber}
        </span>
        <span className="block truncate text-[11px] text-muted-foreground">
          {version.gameVersions.join(', ')} ·{' '}
          {agoLabel(Date.parse(version.datePublished) / 1000)}
        </span>
      </span>
      {version.channel !== 'release' && (
        <Badge variant="outline" className="shrink-0 text-[10px]">
          {version.channel}
        </Badge>
      )}
      {current && (
        <Badge variant="secondary" className="shrink-0 text-[10px]">
          {m['content.current_version']()}
        </Badge>
      )}
    </button>
  );
}
