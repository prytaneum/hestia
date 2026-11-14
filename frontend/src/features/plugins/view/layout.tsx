import { useQuery } from '@tanstack/react-query';

import type { ImageShape, Tone } from '@/api/types/ui';
import { Markdown } from '@/components/markdown';
import { Badge } from '@/components/ui/badge';
import { Progress } from '@/components/ui/progress';
import { Notice as WarningBox } from '@/components/warning-notice';
import { pluginAsset } from '@/lib/plugin-asset';
import { cn } from '@/lib/utils';
import { pluginQueries } from '@/queries/plugin';

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
  if (tone === 'warning' || tone === 'error') {
    return <WarningBox message={text} />;
  }
  return (
    <div
      className={cn(
        'border border-border bg-muted/40 px-3 py-2.5 text-xs leading-relaxed',
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

/** From a host the plugin's grant names: the daemon fetches it and refuses any other, so the webview never contacts the host. */
export function RemoteImage({
  url,
  alt,
  shape,
}: {
  url: string;
  alt: string;
  shape: ImageShape;
}) {
  const { plugin } = useView();
  const image = useQuery(pluginQueries.image(plugin, url));
  const avatar = shape === 'avatar';
  const size = avatar ? 'size-8 rounded-full' : 'max-w-full';
  if (!image.data) {
    return (
      <span
        role="img"
        aria-label={alt}
        className={cn('inline-block bg-muted', avatar ? size : 'h-16 w-16')}
      />
    );
  }
  return (
    <img
      src={`data:${image.data.mime};base64,${image.data.bytes}`}
      alt={alt}
      className={cn(
        'inline-block object-cover',
        size,
        avatar && '[image-rendering:pixelated]',
      )}
    />
  );
}

const BADGE: Record<Tone, string> = {
  neutral: 'bg-secondary text-secondary-foreground',
  muted: 'border-border text-muted-foreground',
  info: 'bg-primary/10 text-foreground',
  success: 'bg-emerald-500/15 text-emerald-500',
  warning: 'bg-amber/15 text-amber',
  error: 'bg-destructive/10 text-destructive',
};

export function PluginBadge({ text, tone }: { text: string; tone: Tone }) {
  return <Badge className={BADGE[tone]}>{text}</Badge>;
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
