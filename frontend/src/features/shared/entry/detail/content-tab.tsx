import { PlusIcon } from '@phosphor-icons/react';

import { Button } from '@/components/ui/button';
import { TabsContent } from '@/components/ui/tabs';
import type { useEntryInstall } from '@/features/content/install';
import { m } from '@/paraglide/messages.js';
import type { SectionProps } from '../lib/content';
import { ContentSection } from './content-section';

export function EntryContentTab({
  install,
  ...section
}: Omit<SectionProps, 'action'> & {
  install: ReturnType<typeof useEntryInstall>;
}) {
  return (
    <TabsContent
      {...install.getRootProps()}
      value="content"
      className="flex flex-col p-5"
    >
      <ContentSection
        {...section}
        action={
          <Button
            size="sm"
            variant="outline"
            data-icon="inline-start"
            onClick={install.start}
          >
            <PlusIcon weight="bold" />
            {m['content.add']()}
          </Button>
        }
      />
    </TabsContent>
  );
}
