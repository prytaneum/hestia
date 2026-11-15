import { CheckIcon, CopyIcon, WarningIcon } from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import { m } from '@/paraglide/messages.js';

const RESET_MS = 2000;

type CopyState = 'idle' | 'copied' | 'failed';

export function CopyValue({ text, label }: { text: string; label?: string }) {
  const [state, setState] = useState<CopyState>('idle');
  const timer = useRef<number>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = async () => {
    window.clearTimeout(timer.current);
    try {
      await navigator.clipboard.writeText(text);
      setState('copied');
    } catch {
      setState('failed');
    }
    timer.current = window.setTimeout(() => setState('idle'), RESET_MS);
  };

  const caption = {
    idle: m['plugins.copy.action'](),
    copied: m['app.status.copied'](),
    failed: m['plugins.copy.failed'](),
  }[state];
  const Icon = { idle: CopyIcon, copied: CheckIcon, failed: WarningIcon }[
    state
  ];

  return (
    <div className="flex min-w-0 items-center gap-2">
      {label && <span className="text-xs text-muted-foreground">{label}</span>}
      <code className="min-w-0 flex-1 select-text truncate border border-border bg-muted/40 px-2 py-1 font-mono text-xs">
        {text}
      </code>
      <Button
        type="button"
        variant="outline"
        size="icon-sm"
        aria-label={caption}
        title={caption}
        onClick={() => void copy()}
      >
        <Icon aria-hidden />
      </Button>
      <span role="status" className="sr-only">
        {state === 'idle' ? '' : caption}
      </span>
    </div>
  );
}
