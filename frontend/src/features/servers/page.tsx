import { PlusIcon } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';
import { useSearch } from '@/components/app-shell/search-context';
import { FilterMenu } from '@/components/filter-menu';
import { Page } from '@/components/page';
import { Button } from '@/components/ui/button';
import {
  EntryCollection,
  EntryGridSkeleton,
  filterCards,
  flavorGroup,
  flavorsOf,
  type View,
  ViewToggle,
} from '@/features/shared/entry/components';
import { CreateEntryDialog } from '@/features/shared/entry/dialogs';
import { useServerCards } from '@/features/shared/entry/hooks/use-entry-cards';
import { m } from '@/paraglide/messages.js';

export function ServersPage({
  view,
  flavor,
  onViewChange,
  onFlavorChange,
}: {
  view: View;
  flavor: string;
  onViewChange: (view: View) => void;
  onFlavorChange: (flavor: string) => void;
}) {
  const { query } = useSearch();
  const { cards, isPending } = useServerCards();
  const [creating, setCreating] = useState(false);

  const flavors = useMemo(() => flavorsOf(cards), [cards]);
  const filtered = filterCards(cards, query, flavor);

  return (
    <Page
      title={m['app.nav.servers']()}
      subtitle={m['server.subtitle']()}
      skeleton={<EntryGridSkeleton />}
      loading={isPending}
      search
      searchPlaceholder={m['app.search.servers']()}
      actions={
        <>
          <FilterMenu
            groups={[flavorGroup(flavors, flavor, onFlavorChange)]}
            label={m['app.collection.filter_by_flavor']()}
          />
          <ViewToggle view={view} onView={onViewChange} />
          <Button
            size="sm"
            data-icon="inline-start"
            onClick={() => setCreating(true)}
          >
            <PlusIcon weight="bold" />
            {m['server.new']()}
          </Button>
        </>
      }
    >
      <EntryCollection
        cards={filtered}
        view={view}
        empty={m['server.none_match']()}
      />
      <CreateEntryDialog
        kind="server"
        open={creating}
        onOpenChange={setCreating}
      />
    </Page>
  );
}
