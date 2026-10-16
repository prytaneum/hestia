import { FileArrowUpIcon, PlusIcon } from '@phosphor-icons/react';
import { Link } from '@tanstack/react-router';
import { useCallback, useMemo, useState } from 'react';
import { dialog } from '@/api';
import { useSearch } from '@/components/app-shell/search-context';
import { FilterMenu } from '@/components/filter-menu';
import { entryIcon } from '@/components/icons';
import { Page, Section } from '@/components/page';
import { SignInGate } from '@/components/sign-in-gate';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ImportInstanceDialog } from '@/features/instances/dialogs';
import { useOpenedArchive } from '@/features/instances/hooks';
import { anchor, GettingStarted, useAutoTour } from '@/features/onboarding';
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
import {
  useInstanceCards,
  useServerCards,
} from '@/features/shared/entry/hooks/use-entry-cards';
import { useDropzone } from '@/hooks/use-dropzone';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';

const InstanceIcon = entryIcon('instance');
const ServerIcon = entryIcon('server');

export function LibraryPage({
  view,
  serverFlavor,
  instanceFlavor,
  onViewChange,
  onServerFlavorChange,
  onInstanceFlavorChange,
}: {
  view: View;
  serverFlavor: string;
  instanceFlavor: string;
  onViewChange: (view: View) => void;
  onServerFlavorChange: (flavor: string) => void;
  onInstanceFlavorChange: (flavor: string) => void;
}) {
  const { query } = useSearch();
  const { signedIn, ready } = useAccounts();

  const { cards: serverCards, isPending: serversPending } = useServerCards();
  const { cards: instanceCards, isPending: instancesPending } =
    useInstanceCards();

  const [newKind, setNewKind] = useState<'server' | 'instance'>('instance');
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [dropped, setDropped] = useState('');
  // A dropped archive opens the import dialog on it; signing in is what an
  // instance needs, so a drop before that would create something unusable.
  const openImport = useCallback(
    (path: string) => {
      if (!signedIn) return;
      setDropped(path);
      setImporting(true);
    },
    [signedIn],
  );
  const { getRootProps } = useDropzone<HTMLDivElement>({
    accept: dialog.ARCHIVE_EXTENSIONS,
    multiple: false,
    disabled: !signedIn,
    label: m['instance.import.drop'](),
    onDrop: ([archive]) => openImport(archive),
  });
  // A `.hestia` file opened from the file manager lands here too — the shell
  // hands it over, and this is the page that can answer it.
  useOpenedArchive(openImport);
  const openNew = (kind: 'server' | 'instance') => {
    setNewKind(kind);
    setCreating(true);
  };

  const serverFlavors = useMemo(() => flavorsOf(serverCards), [serverCards]);
  const instanceFlavors = useMemo(
    () => flavorsOf(instanceCards),
    [instanceCards],
  );

  const srv = filterCards(serverCards, query, serverFlavor);
  const inst = filterCards(instanceCards, query, instanceFlavor);

  const loading = !ready || serversPending || (signedIn && instancesPending);
  useAutoTour('shell', !loading);

  return (
    <Page
      {...getRootProps()}
      title={m['app.nav.library']()}
      subtitle={m['library.subtitle']()}
      loading={loading}
      skeleton={
        <div className="flex flex-col gap-6">
          <EntryGridSkeleton header count={4} />
          <EntryGridSkeleton header count={4} />
        </div>
      }
      search
      searchPlaceholder={m['app.search.library']()}
      actions={
        <>
          <ViewToggle view={view} onView={onViewChange} />
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  size="sm"
                  data-icon="inline-start"
                  {...anchor('library-new')}
                >
                  <PlusIcon weight="bold" />
                  {m['library.new']()}
                </Button>
              }
            />
            <DropdownMenuContent align="end" className="w-44">
              <DropdownMenuItem
                disabled={!signedIn}
                onClick={() => openNew('instance')}
              >
                <InstanceIcon />
                {m['instance.new']()}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => openNew('server')}>
                <ServerIcon />
                {m['server.new']()}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                disabled={!signedIn}
                onClick={async () => {
                  const [archive] = await dialog.pickInstanceArchive();
                  if (archive) openImport(archive);
                }}
              >
                <FileArrowUpIcon />
                {m['instance.import.action']()}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      }
    >
      <div className="flex flex-col gap-6">
        <GettingStarted onCreate={openNew} />

        <Section
          title={m['app.nav.instances']()}
          count={signedIn ? inst.length : undefined}
          action={
            signedIn ? (
              <div className="flex items-center gap-3">
                <FilterMenu
                  groups={[
                    flavorGroup(
                      instanceFlavors,
                      instanceFlavor,
                      onInstanceFlavorChange,
                    ),
                  ]}
                  label={m['app.collection.filter_by_flavor']()}
                />
                <Link
                  to="/instances"
                  className="text-xs text-muted-foreground hover:text-foreground"
                >
                  {m['library.manage_all']()}
                </Link>
              </div>
            ) : undefined
          }
        >
          {signedIn ? (
            <EntryCollection
              cards={inst}
              view={view}
              empty={m['instance.none_match']()}
            />
          ) : (
            <InstancesSignInPrompt />
          )}
        </Section>

        <Section
          title={m['library.your_servers']()}
          count={srv.length}
          action={
            <div className="flex items-center gap-3">
              <FilterMenu
                groups={[
                  flavorGroup(
                    serverFlavors,
                    serverFlavor,
                    onServerFlavorChange,
                  ),
                ]}
                label={m['app.collection.filter_by_flavor']()}
              />
              <Link
                to="/servers"
                className="text-xs text-muted-foreground hover:text-foreground"
              >
                {m['library.manage_all']()}
              </Link>
            </div>
          }
        >
          <EntryCollection
            cards={srv}
            view={view}
            empty={m['server.none_match']()}
          />
        </Section>
      </div>

      <CreateEntryDialog
        kind={newKind}
        open={creating}
        onOpenChange={setCreating}
        onImport={openImport}
      />
      <ImportInstanceDialog
        open={importing}
        onOpenChange={setImporting}
        initialPath={dropped}
      />
    </Page>
  );
}

/** Instances need a signed-in account, so their section blocks until sign-in. */
function InstancesSignInPrompt() {
  return (
    <SignInGate
      className="h-auto py-10"
      title={m['account.sign_in_to_play']()}
      hint={m['instance.sign_in_hint']()}
    />
  );
}
