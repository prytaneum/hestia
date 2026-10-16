import { PlusIcon } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';
import { dialog } from '@/api';
import { useSearch } from '@/components/app-shell/search-context';
import { FilterMenu } from '@/components/filter-menu';
import { Page } from '@/components/page';
import { SignInGate } from '@/components/sign-in-gate';
import { Button } from '@/components/ui/button';
import { ImportInstanceDialog } from '@/features/instances/dialogs';
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
import { useInstanceCards } from '@/features/shared/entry/hooks/use-entry-cards';
import { useDropzone } from '@/hooks/use-dropzone';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';

export function InstancesPage({
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
  const { signedIn, ready } = useAccounts();
  const { cards, isPending } = useInstanceCards();
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [dropped, setDropped] = useState('');
  const openImport = (archive: string) => {
    setDropped(archive);
    setImporting(true);
  };
  const { getRootProps } = useDropzone<HTMLDivElement>({
    accept: dialog.ARCHIVE_EXTENSIONS,
    multiple: false,
    disabled: !signedIn,
    label: m['instance.import.drop'](),
    onDrop: ([archive]) => openImport(archive),
  });

  const flavors = useMemo(() => flavorsOf(cards), [cards]);
  const filtered = filterCards(cards, query, flavor);

  return (
    <Page
      {...getRootProps()}
      title={m['app.nav.instances']()}
      subtitle={m['instance.subtitle']()}
      loading={!ready || (signedIn && isPending)}
      skeleton={<EntryGridSkeleton />}
      search={signedIn}
      searchPlaceholder={m['app.search.instances']()}
      actions={
        signedIn ? (
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
              {m['instance.new']()}
            </Button>
          </>
        ) : undefined
      }
    >
      {signedIn ? (
        <>
          <EntryCollection
            cards={filtered}
            view={view}
            empty={m['instance.none_match']()}
          />
          <CreateEntryDialog
            kind="instance"
            open={creating}
            onOpenChange={setCreating}
            onImport={openImport}
          />
          <ImportInstanceDialog
            open={importing}
            onOpenChange={setImporting}
            initialPath={dropped}
          />
        </>
      ) : (
        <SignInGate
          title={m['account.sign_in_to_play']()}
          hint={m['instance.sign_in_hint']()}
        />
      )}
    </Page>
  );
}
