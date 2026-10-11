import { PlusIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { dialog } from '@/api';
import { useSearch } from '@/components/app-shell/search-context';
import { FilterMenu } from '@/components/filter-menu';
import { Page } from '@/components/page';
import { SignInGate } from '@/components/sign-in-gate';
import { Button } from '@/components/ui/button';
import {
  ImportInstanceDialog,
  useLaunchDialog,
} from '@/features/instances/dialogs';
import type { EntryCardModel } from '@/features/shared/entry/components';
import {
  EntryCollection,
  EntryGridSkeleton,
  filterCards,
  flavorGroup,
  flavorsOf,
  instanceToCard,
  type View,
  ViewToggle,
} from '@/features/shared/entry/components';
import { CreateEntryDialog } from '@/features/shared/entry/dialogs';
import { useDropzone } from '@/hooks/use-dropzone';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { instanceMutations, useInstances } from '@/queries/instance';

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
  const instances = useInstances();
  const { launch, isLaunching } = useLaunchDialog();
  const stop = useMutation(instanceMutations.stopAny());
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [dropped, setDropped] = useState('');
  const { getRootProps } = useDropzone<HTMLDivElement>({
    accept: dialog.ARCHIVE_EXTENSIONS,
    multiple: false,
    disabled: !signedIn,
    label: m['instance.import.drop'](),
    onDrop: ([archive]) => {
      setDropped(archive);
      setImporting(true);
    },
  });

  const cards: EntryCardModel[] = useMemo(
    () =>
      (instances.data ?? []).map((instance) => ({
        ...instanceToCard(
          instance,
          {
            busy:
              isLaunching(instance.id) ||
              (stop.isPending && stop.variables?.id === instance.id),
            launching: isLaunching(instance.id),
            stopping: stop.isPending && stop.variables?.id === instance.id,
            onStart: () => launch(instance),
            onStop: (session) => stop.mutate({ id: instance.id, session }),
            onNewSession: () => launch(instance, { newSession: true }),
          },
          instance.lastPlayedUnix,
        ),
        iconUrl: instance.iconUrl,
      })),
    [instances.data, isLaunching, launch, stop],
  );

  const flavors = useMemo(() => flavorsOf(cards), [cards]);
  const filtered = filterCards(cards, query, flavor);

  return (
    <Page
      {...getRootProps()}
      title={m['app.nav.instances']()}
      subtitle={m['instance.subtitle']()}
      loading={!ready || (signedIn && instances.isPending)}
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
            onImport={() => {
              setDropped('');
              setImporting(true);
            }}
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
