import { cn, slots } from '@/lib/utils';

/** The pulsing placeholder primitive; size it with utility classes. */
export function Bone({ className }: { className?: string }) {
  return (
    <div aria-hidden className={cn('animate-pulse bg-muted', className)} />
  );
}

/** Card bones in `grid` — the page's real grid classes, so bones land where cards will. */
export function CardGridSkeleton({
  grid,
  count,
  card,
  header = false,
}: {
  grid: string;
  count: number;
  card: string;
  header?: boolean;
}) {
  return (
    <div>
      {header && <Bone className="mb-3 h-3 w-24" />}
      <div className={grid}>
        {slots(count).map((slot) => (
          <Bone key={slot} className={card} />
        ))}
      </div>
    </div>
  );
}
