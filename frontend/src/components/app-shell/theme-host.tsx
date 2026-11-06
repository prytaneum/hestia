import { useEffect, useRef } from 'react';

import { Button } from '@/components/ui/button';
import { notify } from '@/lib/notify';
import { pluginAsset } from '@/lib/plugin-asset';
import { applyTheme } from '@/lib/theme';
import { m } from '@/paraglide/messages.js';
import { useSafeMode, useTheme } from '@/queries/theme';

/**
 * Applies the chosen plugin theme, and in safe mode applies none and says so.
 * A stylesheet that fails to load turns the choice off rather than leaving a
 * half-styled app behind.
 */
export function ThemeHost() {
  const safe = useSafeMode();
  const { active, select } = useTheme();
  const deselect = useRef(select);
  deselect.current = select;

  useEffect(() => {
    if (!active || safe !== false) return;
    return applyTheme(
      pluginAsset(active.plugin, active.file),
      active.appearance === 'light',
      () => {
        deselect.current(null);
        notify.error(m['settings.theme.failed']({ name: active.name }));
      },
    );
  }, [active, safe]);

  if (!safe) return null;
  return (
    <div className="flex items-center justify-between gap-4 border-b border-border bg-muted px-4 py-2 text-sm">
      <span>{m['app.safe_mode.notice']()}</span>
      <Button variant="outline" size="sm" onClick={() => select(null)}>
        {m['app.safe_mode.reset']()}
      </Button>
    </div>
  );
}
