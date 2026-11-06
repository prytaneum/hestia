import type { DescribedSection } from '@/features/settings/lib';
import { m } from '@/paraglide/messages.js';

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
    <div className="space-y-2">
      {sections.map((section) => (
        <div key={section.kind}>
          <div className="text-xs font-medium">{section.heading}</div>
          <ul className="text-[11px] text-muted-foreground">
            {section.items.map((item) => (
              <li key={item.label}>
                <span className="font-mono">{item.label}</span>
                {item.detail && ` — ${item.detail}`}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}
