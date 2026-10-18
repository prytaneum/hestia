import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import {
  type ContentMatch,
  type ContentVersion,
  content as contentApi,
  type InstalledContent,
} from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { OfflineState } from '@/components/offline-state';
import { SearchInput } from '@/components/search-input';
import { CardGridSkeleton } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { m } from '@/paraglide/messages.js';
import { contentQueries } from '@/queries/content';
import { useOffline } from '@/queries/net';
import { filterVersions } from '../lib';
import { VersionOption } from './version-option';

/**
 * Pick a specific published version of an installed item — real
 * `content.versions`, presented like the create wizard's version picker
 * (search over a selectable list). The re-pin applies at the next launch.
 */
export function ContentVersionDialog({
  item,
  loader,
  gameVersion,
  matches,
  matchesLoading,
  onOpenChange,
  onPick,
}: {
  /** The installed item being re-pinned, or `null` when closed. */
  item: InstalledContent | null;
  loader?: string;
  gameVersion?: string;
  matches: ContentMatch[];
  matchesLoading: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (item: InstalledContent, version: ContentVersion) => void;
}) {
  const [search, setSearch] = useState('');
  const [pickedId, setPickedId] = useState<string | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);

  const names = useQuery(contentQueries.sources()).data ?? [];
  const tracked = !!item?.projectId && item.source !== contentApi.LOCAL_SOURCE;
  const sources = [
    ...(tracked && item
      ? [{ source: item.source, project: item.projectId, title: item.title }]
      : []),
    ...matches
      .filter((match) => !tracked || match.project.source !== item?.source)
      .map(({ project }) => ({
        source: project.source,
        project: project.id,
        title: project.title,
      })),
  ];
  const active = sources.find((s) => s.source === chosen) ?? sources[0];
  const moving = !!active && active.source !== item?.source;
  const options = sources.map((s) => ({
    value: s.source,
    label: `${names.find((n) => n.id === s.source)?.name ?? s.source} · ${s.title}`,
  }));

  const versions = useQuery(
    contentQueries.versions({
      source: active?.source ?? '',
      project: active?.project ?? '',
      loader,
      gameVersion,
    }),
  );
  const list = item ? (versions.data ?? []) : [];
  const offline = useOffline();
  const shown = filterVersions(list, search);
  const picked = list.find((v) => v.id === pickedId) ?? null;

  const close = (next: boolean) => {
    if (!next) {
      setSearch('');
      setPickedId(null);
      setChosen(null);
    }
    onOpenChange(next);
  };

  return (
    <AppDialog
      open={item !== null}
      onOpenChange={close}
      title={m['content.change_version.title']({ name: item?.title ?? '' })}
      description={m['content.change_version.description']()}
    >
      <DialogBody className="flex flex-col gap-3">
        {(options.length > 1 || (moving && options.length > 0)) && (
          <div className="flex items-center gap-2">
            <span className="shrink-0 text-xs text-muted-foreground">
              {m['content.browse.source']()}
            </span>
            <Select
              items={options}
              value={active?.source}
              onValueChange={(value) => {
                setChosen(String(value));
                setPickedId(null);
              }}
            >
              <SelectTrigger size="sm" className="min-w-0 flex-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {options.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
        <SearchInput
          value={search}
          onChange={setSearch}
          placeholder={m['entry.create.filter_versions']()}
        />

        <div className="max-h-64 divide-y divide-border overflow-y-auto border border-border">
          {offline && shown.length === 0 ? (
            <OfflineState />
          ) : !active && !matchesLoading ? (
            <p className="px-3 py-6 text-center text-xs text-muted-foreground">
              {m['content.change_version.unrecognized']()}
            </p>
          ) : versions.isPending ? (
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
                selected={pickedId === v.id}
                current={!moving && v.versionNumber === item?.versionNumber}
                onSelect={() => setPickedId(v.id)}
              />
            ))
          )}
        </div>
      </DialogBody>

      <DialogFooter>
        <Button variant="outline" onClick={() => close(false)}>
          {m['app.action.cancel']()}
        </Button>
        <Button
          disabled={
            !picked || (!moving && picked.versionNumber === item?.versionNumber)
          }
          onClick={() => {
            if (item && picked) onPick(item, picked);
            close(false);
          }}
        >
          {m['app.action.apply']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}
