import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import {
  type ContentMatch,
  type ContentVersion,
  content as contentApi,
  type InstalledContent,
} from '@/api';
import { OfflineState } from '@/components/offline-state';
import { SearchInput } from '@/components/search-input';
import { CardGridSkeleton } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { agoLabel } from '@/lib/format';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { contentQueries, useContentSources } from '@/queries/content';
import { useOffline } from '@/queries/net';

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

  const names = useContentSources().data ?? [];
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
  const q = search.trim().toLowerCase();
  const shown = list.filter(
    (v) =>
      !q ||
      v.versionNumber.toLowerCase().includes(q) ||
      v.gameVersions.some((g) => g.toLowerCase().includes(q)),
  );
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
    <Dialog open={item !== null} onOpenChange={close}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {m['content.change_version.title']({ name: item?.title ?? '' })}
          </DialogTitle>
          <DialogDescription>
            {m['content.change_version.description']()}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-3">
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
              shown.map((v) => {
                const current =
                  !moving && v.versionNumber === item?.versionNumber;
                const selected = pickedId === v.id;
                return (
                  <button
                    key={v.id}
                    type="button"
                    aria-pressed={selected}
                    onClick={() => setPickedId(v.id)}
                    className={cn(
                      'flex w-full items-center gap-2.5 px-3 py-2 text-left outline-none transition-colors focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset',
                      selected
                        ? 'bg-muted text-foreground'
                        : 'hover:bg-muted/50',
                    )}
                  >
                    <span
                      className={cn(
                        'size-1.5 shrink-0 rounded-full',
                        selected ? 'bg-ember' : 'bg-transparent',
                      )}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-mono text-xs">
                        {v.versionNumber}
                      </span>
                      <span className="block truncate text-[11px] text-muted-foreground">
                        {v.gameVersions.join(', ')} ·{' '}
                        {agoLabel(Date.parse(v.datePublished) / 1000)}
                      </span>
                    </span>
                    {v.channel !== 'release' && (
                      <Badge variant="outline" className="shrink-0 text-[10px]">
                        {v.channel}
                      </Badge>
                    )}
                    {current && (
                      <Badge
                        variant="secondary"
                        className="shrink-0 text-[10px]"
                      >
                        {m['content.current_version']()}
                      </Badge>
                    )}
                  </button>
                );
              })
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => close(false)}>
            {m['app.action.cancel']()}
          </Button>
          <Button
            disabled={
              !picked ||
              (!moving && picked.versionNumber === item?.versionNumber)
            }
            onClick={() => {
              if (item && picked) onPick(item, picked);
              close(false);
            }}
          >
            {m['app.action.apply']()}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
