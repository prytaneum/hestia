import { ShieldWarningIcon } from '@phosphor-icons/react';

import type { DescribedSection } from '@/features/settings/lib';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

/**
 * What a plugin is allowed, section by section. Full access leads in the warning
 * tone, since it is the part of a grant worth reading twice.
 */
export function GrantList({ sections }: { sections: DescribedSection[] }) {
  if (sections.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">
        {m['settings.plugins.no_permissions']()}
      </p>
    );
  }
  return (
    <div className="space-y-3">
      {sections.map((section) => {
        const full = section.kind === 'full_access';
        return (
          <section key={section.kind} className="space-y-1.5">
            <h4
              className={cn(
                'flex items-center gap-1.5 text-xs font-medium',
                full && 'text-amber',
              )}
            >
              {full && <ShieldWarningIcon weight="fill" className="size-3.5" />}
              {section.heading}
            </h4>
            <ul className="space-y-1">
              {section.items.map((item) => (
                <li
                  key={item.label}
                  className="grid grid-cols-[minmax(7rem,auto)_1fr] gap-x-3 text-xs"
                >
                  <span
                    className={cn(
                      'font-mono text-[11px]',
                      full ? 'text-amber' : 'text-foreground/80',
                    )}
                  >
                    {item.label}
                  </span>
                  <span className="text-muted-foreground">{item.detail}</span>
                </li>
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}
