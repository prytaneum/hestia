import { type ReactNode, useState } from 'react';

import { FieldGroup } from '@/components/ui/field';
import { Tabs, TabsChip, TabsContent, TabsList } from '@/components/ui/tabs';

export interface SettingsTabDef {
  id: string;
  label: string;
  content: ReactNode;
}

/**
 * An entry's settings, one area at a time. The areas are chips rather than
 * line tabs so they read as a level below the entry's own tab row.
 */
export function SettingsTabs({ tabs }: { tabs: SettingsTabDef[] }) {
  const [active, setActive] = useState(tabs[0].id);

  return (
    <Tabs
      value={active}
      onValueChange={(value) => setActive(value as string)}
      className="gap-0"
    >
      <TabsList
        variant="line"
        className="h-auto min-h-8 flex-wrap justify-start gap-1.5"
      >
        {tabs.map((tab) => (
          <TabsChip key={tab.id} value={tab.id}>
            {tab.label}
          </TabsChip>
        ))}
      </TabsList>
      <div className="max-w-2xl pt-3 pb-6">
        {tabs.map((tab) => (
          <TabsContent key={tab.id} value={tab.id}>
            <FieldGroup className="gap-8">{tab.content}</FieldGroup>
          </TabsContent>
        ))}
      </div>
    </Tabs>
  );
}
