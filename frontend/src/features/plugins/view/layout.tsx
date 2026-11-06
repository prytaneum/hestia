import type { Tone } from '@/api/types/ui';
import { Markdown } from '@/components/markdown';
import { Progress } from '@/components/ui/progress';
import { pluginAsset } from '@/lib/plugin-asset';
import { cn } from '@/lib/utils';

import { useView } from './context';

const TONE: Record<Tone, string> = {
  neutral: '',
  muted: 'text-muted-foreground',
  info: 'text-foreground/80',
  success: 'text-emerald-500',
  warning: 'text-amber',
  error: 'text-destructive',
};

export function Heading({ text, level }: { text: string; level: number }) {
  if (level <= 1) {
    return <h2 className="font-heading text-lg font-semibold">{text}</h2>;
  }
  if (level === 2) {
    return <h3 className="font-heading text-base font-semibold">{text}</h3>;
  }
  return <h4 className="text-sm font-semibold">{text}</h4>;
}

export function Text({ text, tone }: { text: string; tone: Tone }) {
  return <p className={cn('text-sm', TONE[tone])}>{text}</p>;
}

export function Notice({ text, tone }: { text: string; tone: Tone }) {
  return (
    <div
      className={cn(
        'border border-border bg-muted/40 px-3 py-2.5 text-xs leading-relaxed',
        tone === 'warning' && 'border-amber/40 bg-amber/5',
        tone === 'error' && 'border-destructive/40 bg-destructive/5',
        TONE[tone],
      )}
    >
      {text}
    </div>
  );
}

export function Image({ src, alt }: { src: string; alt: string }) {
  const { plugin } = useView();
  return (
    <img
      src={pluginAsset(plugin, src)}
      alt={alt}
      className="h-auto max-w-full"
    />
  );
}

export function Markup({ source }: { source: string }) {
  return <Markdown raw={false}>{source}</Markdown>;
}

export function Meter({
  fraction,
  label,
}: {
  fraction?: number;
  label?: string;
}) {
  const value = fraction == null ? null : Math.min(Math.max(fraction, 0), 1);
  return (
    <div className="space-y-1">
      {label && <p className="text-xs text-muted-foreground">{label}</p>}
      <Progress value={value == null ? null : value * 100} />
    </div>
  );
}
