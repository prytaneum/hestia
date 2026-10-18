import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import {
  type ContentVersion,
  downgradeBetween,
  type EntryKind,
  type InstalledModpack,
} from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { OfflineState } from '@/components/offline-state';
import { SearchInput } from '@/components/search-input';
import { CardGridSkeleton } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { VersionOption } from '@/features/shared/content/dialogs';
import { filterVersions } from '@/features/shared/content/lib';
import { useOnOpen } from '@/features/shared/entry/hooks';
import { m } from '@/paraglide/messages.js';
import { catalogueQueries } from '@/queries/catalogue';
import { contentQueries } from '@/queries/content';
import { useOffline } from '@/queries/net';

export function ModpackVersionDialog({
  pack,
  kind,
  flavor,
  gameVersion,
  preselect,
  open,
  onOpenChange,
  onPick,
}: {
  pack: InstalledModpack;
  kind: EntryKind;
  flavor: string;
  gameVersion: string;
  preselect?: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (version: ContentVersion, allowDowngrade: boolean) => void;
}) {
  const [search, setSearch] = useState('');
  const [chosen, setChosen] = useState<string | null>(null);
  const [downgrade, setDowngrade] = useState(false);

  useOnOpen(open, () => {
    setSearch('');
    setChosen(null);
    setDowngrade(false);
  });

  const published = useQuery({
    ...contentQueries.versions({
      source: pack.source,
      project: pack.projectId,
    }),
    enabled: open && pack.projectId !== '',
  });
  const catalogue = useQuery({
    ...catalogueQueries.versions(kind, flavor),
    enabled: open,
  });
  const offline = useOffline();

  const list = published.data ?? [];
  const shown = filterVersions(list, search);
  const picked = list.find((v) => v.id === (chosen ?? preselect)) ?? null;
  const goesBack =
    !!picked &&
    picked.gameVersions.length > 0 &&
    picked.gameVersions.every(
      (g) => downgradeBetween(catalogue.data ?? [], gameVersion, g) === true,
    );

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      title={m['content.change_version.title']({ name: pack.name })}
      description={m['content.modpack.change_version_description']()}
    >
      <DialogBody className="flex flex-col gap-3">
        <SearchInput
          value={search}
          onChange={setSearch}
          placeholder={m['entry.create.filter_versions']()}
        />
        <div className="max-h-64 divide-y divide-border overflow-y-auto border border-border">
          {offline && shown.length === 0 ? (
            <OfflineState />
          ) : published.isPending ? (
            <CardGridSkeleton
              grid="divide-y divide-border"
              count={4}
              card="h-14"
            />
          ) : shown.length === 0 ? (
            <p className="px-3 py-6 text-center text-xs text-muted-foreground">
              {m['entry.create.no_versions_match']()}
            </p>
          ) : (
            shown.map((v) => (
              <VersionOption
                key={v.id}
                version={v}
                selected={picked?.id === v.id}
                current={v.id === pack.versionId}
                onSelect={() => {
                  setChosen(v.id);
                  setDowngrade(false);
                }}
              />
            ))
          )}
        </div>
        {goesBack && (
          <label
            htmlFor="pack-allow-downgrade"
            className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground"
          >
            <Checkbox
              id="pack-allow-downgrade"
              checked={downgrade}
              onCheckedChange={(c) => setDowngrade(c === true)}
            />
            {m['entry.settings.allow_downgrade']()}
          </label>
        )}
      </DialogBody>

      <DialogFooter>
        <Button variant="outline" onClick={() => onOpenChange(false)}>
          {m['app.action.cancel']()}
        </Button>
        <Button
          disabled={
            !picked || picked.id === pack.versionId || (goesBack && !downgrade)
          }
          onClick={() => {
            if (picked) onPick(picked, goesBack);
            onOpenChange(false);
          }}
        >
          {m['app.action.apply']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}
