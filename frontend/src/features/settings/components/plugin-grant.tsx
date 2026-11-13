import type { DescribedSection, GrantScope } from '@/features/settings/lib';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

function ScopeList({ scopes }: { scopes: GrantScope[] }) {
  return scopes.map((scope) => (
    <div key={scope.heading + scope.entries.join()} className="space-y-0.5">
      {scope.heading && (
        <div className="text-[11px] text-muted-foreground">{scope.heading}</div>
      )}
      <ul className="space-y-0.5 text-[11px]">
        {scope.entries.map((entry) => (
          <li key={entry} className="break-all font-mono text-foreground/80">
            {entry}
          </li>
        ))}
      </ul>
    </div>
  ));
}

/** What a plugin is allowed, section by section, full access first. */
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
      {sections.map((section) => (
        <div key={section.kind} className="space-y-1">
          <div
            className={cn(
              'text-xs font-medium',
              section.kind === 'full_access' && 'text-amber',
            )}
          >
            {section.heading}
          </div>
          <dl className="grid grid-cols-[10rem_1fr] gap-x-4 gap-y-1 text-[11px]">
            {section.items.map((item) => (
              <div key={item.label} className="contents">
                <dt className="truncate font-mono text-foreground/80">
                  {item.label}
                </dt>
                <dd className="space-y-1 text-muted-foreground">
                  {item.detail}
                  <ScopeList scopes={item.scopes} />
                </dd>
              </div>
            ))}
          </dl>
          <ScopeList scopes={section.scopes} />
        </div>
      ))}
    </div>
  );
}
