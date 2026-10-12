import { type ReactNode, useState } from 'react';

import { FieldGroup } from '@/components/ui/field';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';

export interface SettingsTabDef {
  id: string;
  label: string;
  content: ReactNode;
}

/**
 * An entry's settings, one tab per area — laid out as the launcher's own
 * settings page is, so the two read as the same surface.
 */
export function SettingsTabs({ tabs }: { tabs: SettingsTabDef[] }) {
  const [active, setActive] = useState(tabs[0].id);

  return (
    <Tabs
      value={active}
      onValueChange={(value) => setActive(value as string)}
      className="gap-0"
    >
      <TabsList variant="line" className="h-auto gap-6 border-b border-border">
        {tabs.map((tab) => (
          <TabsTrigger key={tab.id} value={tab.id}>
            {tab.label}
          </TabsTrigger>
        ))}
      </TabsList>
      <div className="max-w-2xl py-6">
        {tabs.map((tab) => (
          <TabsContent key={tab.id} value={tab.id}>
            <FieldGroup className="gap-8">{tab.content}</FieldGroup>
          </TabsContent>
        ))}
      </div>
    </Tabs>
  );
}
