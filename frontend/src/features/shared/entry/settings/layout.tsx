import type { Icon } from '@phosphor-icons/react';
import { type ReactNode, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

export interface SettingsSectionDef {
  id: string;
  label: string;
  icon: Icon;
  content: ReactNode;
  /** Sets the entry apart from the rest of the list (the danger zone). */
  destructive?: boolean;
}

/** A section list on the left, one section's body on the right. */
export function SettingsLayout({
  sections,
}: {
  sections: SettingsSectionDef[];
}) {
  const [active, setActive] = useState(sections[0].id);

  return (
    <Tabs
      orientation="vertical"
      value={active}
      onValueChange={(value) => setActive(value as string)}
      className="h-full min-h-0 gap-0"
    >
      <TabsList
        variant="line"
        className="h-full w-44 shrink-0 items-stretch justify-start gap-0.5 border-r border-border p-3 group-data-vertical/tabs:h-full"
      >
        {sections.map(({ id, label, icon: SectionIcon, destructive }) => (
          <TabsTrigger
            key={id}
            value={id}
            className={cn(
              'flex-none justify-start gap-2 px-2.5 py-1.5 after:hidden group-data-[variant=line]/tabs-list:py-1.5 group-data-[variant=line]/tabs-list:data-active:bg-muted dark:group-data-[variant=line]/tabs-list:data-active:bg-muted',
              destructive && 'mt-2 text-destructive hover:text-destructive',
            )}
          >
            <SectionIcon />
            {label}
          </TabsTrigger>
        ))}
      </TabsList>
      {sections.map(({ id, content }) => (
        <TabsContent
          key={id}
          value={id}
          className="flex min-h-0 flex-col overflow-y-auto"
        >
          {content}
        </TabsContent>
      ))}
    </Tabs>
  );
}

export function SettingsSection({
  title,
  description,
  actions,
  className,
  children,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className="flex min-h-0 flex-1 flex-col gap-5 p-5">
      <header className="flex items-start gap-4">
        <div className="min-w-0 flex-1">
          <h2 className="font-heading text-sm font-semibold">{title}</h2>
          {description && (
            <p className="mt-0.5 text-xs text-muted-foreground">
              {description}
            </p>
          )}
        </div>
        {actions}
      </header>
      <div className={cn('max-w-lg', className)}>{children}</div>
    </section>
  );
}

/**
 * The save control for a section edited as a group. Always present so it never
 * shifts the layout, and inert until something differs from what is stored.
 */
export function SaveBar({
  dirty,
  pending,
  invalid = false,
  running = false,
  onSave,
  onDiscard,
}: {
  dirty: boolean;
  pending: boolean;
  invalid?: boolean;
  /** The entry is running, so a saved change waits for the next start. */
  running?: boolean;
  onSave: () => void;
  onDiscard: () => void;
}) {
  return (
    <div className="sticky bottom-0 mt-auto flex items-center gap-3 border-t border-border bg-background px-5 py-3">
      <span
        className={cn(
          'flex-1 text-xs',
          dirty ? 'text-amber' : 'text-muted-foreground',
        )}
      >
        {dirty
          ? m['entry.settings.unsaved']()
          : running
            ? m['entry.settings.applies_on_start']()
            : ''}
      </span>
      <Button
        variant="ghost"
        size="sm"
        disabled={!dirty || pending}
        onClick={onDiscard}
      >
        {m['entry.settings.discard']()}
      </Button>
      <Button
        size="sm"
        disabled={!dirty || pending || invalid}
        onClick={onSave}
      >
        {m['app.action.save']()}
      </Button>
    </div>
  );
}
