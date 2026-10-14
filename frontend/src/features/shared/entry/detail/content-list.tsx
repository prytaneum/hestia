import {
  ArrowsClockwiseIcon,
  DotsThreeIcon,
  FileArrowUpIcon,
  GlobeIcon,
  PackageIcon,
  ProhibitIcon,
  PuzzlePieceIcon,
  SwapIcon,
  TrashIcon,
} from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useState } from 'react';

import { content as contentApi, type InstalledContent } from '@/api';
import { Empty } from '@/components/empty';
import { contentIcon, contentKindLabel } from '@/components/icons';
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Switch } from '@/components/ui/switch';
import { Thumbnail } from '@/components/ui/thumbnail';
import { ContentVersionDialog } from '@/features/shared/content/dialogs';
import { kindInfo } from '@/features/shared/content/lib';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { instanceQueries } from '@/queries/instance';
import { serverQueries } from '@/queries/server';
import { useContent } from '../hooks';
import {
  kindLoader,
  packWorlds,
  parseOrigin,
  rowKey,
  worldEnabled,
} from '../lib';

export function ContentListResult({
  items,
  updatable,
  selected,
  onToggleSelect,
}: {
  items: InstalledContent[];
  updatable: Set<string>;
  selected: Set<string> | null;
  onToggleSelect: (key: string) => void;
}) {
  const { entry, handlers, entryWorlds } = useContent();
  const [changing, setChanging] = useState<InstalledContent | null>(null);
  const isServer = entry.kind === 'server';
  const changingKind = changing?.kind ?? 'mod';
  const changingFile = changing?.filename ?? '';
  const serverMatches = useQuery({
    ...serverQueries.contentMatches(entry.id, changingKind, changingFile),
    enabled: isServer && changingFile.length > 0,
  });
  const instanceMatches = useQuery({
    ...instanceQueries.contentMatches(entry.id, changingKind, changingFile),
    enabled: !isServer && changingFile.length > 0,
  });
  const matches = isServer ? serverMatches : instanceMatches;

  if (items.length === 0) {
    return (
      <Empty className="flex-1" icon={PuzzlePieceIcon}>
        {m['content.none_installed']()}
      </Empty>
    );
  }
  return (
    <>
      <div className="divide-y divide-border border border-border">
        {items.map((c) => (
          <ContentRow
            key={rowKey(c)}
            item={c}
            updatable={updatable.has(c.filename)}
            // Only an instance has many worlds to scope; a server has one, so
            // its datapack row stays a plain row.
            worlds={
              entry.kind === 'instance' && c.kind === 'data_pack'
                ? packWorlds(c, entryWorlds)
                : []
            }
            onChangeVersion={() => setChanging(c)}
            checked={selected ? selected.has(rowKey(c)) : undefined}
            onToggle={() => onToggleSelect(rowKey(c))}
          />
        ))}
      </div>
      <ContentVersionDialog
        item={changing}
        loader={changing ? kindLoader(changing.kind, entry.flavor) : undefined}
        gameVersion={entry.gameVersion || undefined}
        matches={matches.data ?? []}
        matchesLoading={matches.isLoading}
        onOpenChange={(open) => !open && setChanging(null)}
        onPick={(item, version) => handlers.onSetVersion(item, version)}
      />
    </>
  );
}

/**
 * Where a row came from, when it was not installed by hand. The tag names a
 * modpack by project id, so the pack the entry runs supplies the readable name.
 */
function OriginBadge({
  origin,
  packName,
}: {
  origin: string;
  packName: string;
}) {
  if (!parseOrigin(origin)) return null;

  return (
    <Badge
      variant="secondary"
      className="shrink-0 text-muted-foreground"
      title={
        packName
          ? m['content.modpack.origin_badge']({ name: packName })
          : undefined
      }
    >
      <PackageIcon />
      {packName || m['content.modpack.title']()}
    </Badge>
  );
}

function ContentRow({
  item,
  updatable,
  worlds,
  onChangeVersion,
  checked,
  onToggle,
}: {
  item: InstalledContent;
  updatable: boolean;
  /** The worlds this row can scope to; empty for everything but a datapack. */
  worlds: string[];
  onChangeVersion: () => void;
  /** Set while the batch-select mode is active; undefined otherwise. */
  checked?: boolean;
  onToggle: () => void;
}) {
  const { handlers, packName, busy } = useContent();
  const [removing, setRemoving] = useState(false);
  const selecting = checked !== undefined;
  const Icon = contentIcon(item.kind);
  // A local-file import has no project page to open.
  const platform = item.source !== contentApi.LOCAL_SOURCE && !!item.projectId;
  const loadedWorlds = worlds.filter((world) =>
    worldEnabled(item, world),
  ).length;
  const body = (
    <>
      <Thumbnail src={item.iconUrl} icon={Icon} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm group-hover/item:underline group-hover/item:underline-offset-2">
            {item.title}
          </span>
          {!item.enabled && (
            <Badge variant="outline" className="shrink-0">
              {m['content.disabled']()}
            </Badge>
          )}
          <OriginBadge origin={item.origin} packName={packName} />
        </div>
        <div className="truncate font-mono text-[11px] text-muted-foreground">
          {contentKindLabel[item.kind]()} · {item.source} · {item.versionNumber}
        </div>
      </div>
    </>
  );

  if (selecting) {
    const id = `select-${rowKey(item)}`;
    return (
      <label
        htmlFor={id}
        className={cn(
          'flex cursor-pointer items-center gap-3 px-3 py-2.5 transition-colors hover:bg-muted/60',
          !item.enabled && 'opacity-60',
        )}
      >
        <Checkbox id={id} checked={checked} onCheckedChange={onToggle} />
        {body}
      </label>
    );
  }
  // A datapack's worlds nest under it in an accordion whose trigger sits inline
  // among the row's actions, so a pack costs no more list height than a mod.
  return (
    <Accordion className={item.enabled ? undefined : 'opacity-60'}>
      <AccordionItem className="border-b-0">
        <div className="flex items-center gap-3 px-3 py-2.5">
          {platform ? (
            <Link
              to="/browse/$kind/$id"
              params={{
                kind: kindInfo[item.kind].slug,
                id: item.slug || item.projectId,
              }}
              search={{ source: item.source }}
              className="group/item flex min-w-0 flex-1 items-center gap-3 outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              {body}
            </Link>
          ) : (
            <div className="flex min-w-0 flex-1 items-center gap-3">{body}</div>
          )}

          {updatable && item.enabled && (
            <Button
              size="icon-sm"
              variant="outline"
              aria-label={m['content.update_to_latest']()}
              title={m['content.update_to_latest']()}
              disabled={busy}
              onClick={() => handlers.onUpdate(item)}
            >
              <ArrowsClockwiseIcon weight="bold" />
            </Button>
          )}
          {worlds.length > 0 && (
            <AccordionTrigger className="flex-none gap-1.5 py-0 text-muted-foreground hover:no-underline">
              {m['content.loaded_in_worlds']({
                loaded: loadedWorlds,
                total: worlds.length,
              })}
            </AccordionTrigger>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={m['app.action.more']()}
                >
                  <DotsThreeIcon weight="bold" className="size-4" />
                </Button>
              }
            />
            <DropdownMenuContent align="end" className="w-48">
              {platform && (
                <>
                  {updatable && (
                    <DropdownMenuItem
                      disabled={busy}
                      onClick={() => handlers.onUpdate(item)}
                    >
                      <ArrowsClockwiseIcon />
                      {m['content.update_to_latest']()}
                    </DropdownMenuItem>
                  )}
                  <DropdownMenuItem disabled={busy} onClick={onChangeVersion}>
                    <SwapIcon />
                    {m['content.change_version.action']()}
                  </DropdownMenuItem>
                </>
              )}
              {item.source === contentApi.LOCAL_SOURCE && (
                <>
                  {!item.origin && (
                    <DropdownMenuItem
                      disabled={busy}
                      onClick={() => handlers.onUpdateFromFile(item)}
                    >
                      <FileArrowUpIcon />
                      {m['content.update_from_file']()}
                    </DropdownMenuItem>
                  )}
                  <DropdownMenuItem disabled={busy} onClick={onChangeVersion}>
                    <SwapIcon />
                    {m['content.change_version.track_action']()}
                  </DropdownMenuItem>
                </>
              )}
              <DropdownMenuItem
                onClick={() => handlers.onEnable(item, !item.enabled)}
              >
                <ProhibitIcon />
                {item.enabled
                  ? m['app.action.disable']()
                  : m['app.action.enable']()}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="destructive"
                onClick={() => setRemoving(true)}
              >
                <TrashIcon />
                {m['app.action.remove']()}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <ConfirmDialog
            open={removing}
            onOpenChange={setRemoving}
            title={m['content.remove.title']()}
            description={m['content.remove.description']({ name: item.title })}
            destructive
            confirmLabel={m['app.action.remove']()}
            onConfirm={() => {
              setRemoving(false);
              handlers.onRemove(item);
            }}
          />
        </div>
        {worlds.length > 0 && (
          <AccordionContent className="pt-0 pb-0 pl-13">
            <WorldRows item={item} worlds={worlds} />
          </AccordionContent>
        )}
      </AccordionItem>
    </Accordion>
  );
}

/**
 * A datapack loads from inside a world, so its state is per world: the wire
 * keeps one record for the pack and scopes enable/remove by world name. These
 * are that scope — the pack's own row stays a row like any other.
 */
function WorldRows({
  item,
  worlds,
}: {
  item: InstalledContent;
  worlds: string[];
}) {
  const { handlers } = useContent();
  const [removing, setRemoving] = useState<string | null>(null);
  return (
    <div className="flex flex-col pb-1.5">
      {worlds.map((world) => {
        const on = worldEnabled(item, world);
        return (
          <div
            key={world}
            className="group/world flex items-center gap-2 py-1 pr-3"
          >
            <GlobeIcon className="shrink-0 text-muted-foreground" />
            <span
              className={cn('min-w-0 flex-1 truncate', !on && 'opacity-60')}
            >
              {world}
            </span>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={m['app.action.remove']()}
              className="opacity-0 transition-opacity group-hover/world:opacity-100 focus-visible:opacity-100"
              onClick={() => setRemoving(world)}
            >
              <TrashIcon />
            </Button>
            <Switch
              size="sm"
              checked={on}
              aria-label={m['content.loaded_in_world']({ world })}
              onCheckedChange={(next) => handlers.onEnable(item, next, [world])}
            />
          </div>
        );
      })}
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={m['content.remove.from_world_title']()}
        description={m['content.remove.from_world_description']({
          name: item.title,
          world: removing ?? '',
        })}
        destructive
        confirmLabel={m['app.action.remove']()}
        onConfirm={() => {
          const world = removing;
          setRemoving(null);
          if (world) handlers.onRemove(item, [world]);
        }}
      />
    </div>
  );
}
