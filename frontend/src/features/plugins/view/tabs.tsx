import type { UiNode } from '@/api/types/ui';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';

import { keyed } from './keys';
import { UiView } from './render';

/** The chosen tab lives in the component, so a redraw of the page keeps it. */
export function UiTabs({ tabs }: Extract<UiNode, { type: 'tabs' }>) {
  return (
    <Tabs defaultValue={tabs[0]?.id}>
      <TabsList variant="line">
        {tabs.map((tab) => (
          <TabsTrigger key={tab.id} value={tab.id}>
            {tab.label}
          </TabsTrigger>
        ))}
      </TabsList>
      {tabs.map((tab) => (
        <TabsContent key={tab.id} value={tab.id}>
          <div className="flex flex-col gap-4 pt-3">
            {keyed(tab.children).map(({ key, item }) => (
              <UiView key={key} node={item} />
            ))}
          </div>
        </TabsContent>
      ))}
    </Tabs>
  );
}
