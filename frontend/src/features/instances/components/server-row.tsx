import {
  CheckIcon,
  CopyIcon,
  DotsThreeIcon,
  HardDrivesIcon,
  PencilSimpleIcon,
  PlayIcon,
  TrashIcon,
} from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import type { ServerEntry } from '@/api';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Spinner } from '@/components/ui/spinner';
import { StatusDot } from '@/components/ui/status-dot';
import { pngSource, Thumbnail } from '@/components/ui/thumbnail';
import { useSaveState } from '@/hooks/use-save-state';
import { m } from '@/paraglide/messages.js';
import { instanceQueries } from '@/queries/instance';

export function ServerRow({
  server,
  joinable,
  playing,
  onPlay,
  onEdit,
  onRemove,
}: {
  server: ServerEntry;
  joinable: boolean;
  playing: boolean;
  onPlay: () => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const status = useQuery(instanceQueries.serverStatus(server.address));
  const online = status.isSuccess;
  const copy = useSaveState();
  const copied = copy.state === 'saved';

  return (
    <div className="flex items-center gap-3 px-3 py-2.5">
      {/* The live favicon leads: the cached one exists only once the game has
          connected, and goes stale when the server changes its icon. */}
      <Thumbnail
        src={pngSource(status.data?.favicon, server.icon)}
        icon={HardDrivesIcon}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <StatusDot tone={online ? 'on' : 'off'} />
          <span className="truncate text-sm">{server.name}</span>
          {server.acceptTextures && (
            <Badge variant="outline" className="shrink-0">
              {m['instance.servers.textures']()}
            </Badge>
          )}
        </div>
        <div className="truncate font-mono text-[11px] text-muted-foreground">
          {server.address}
          {status.isPending
            ? ` · ${m['instance.servers.checking']()}`
            : online
              ? status.data.motd && ` · ${status.data.motd}`
              : ` · ${m['instance.servers.offline']()}`}
        </div>
      </div>

      {online && (
        <span className="shrink-0 text-[11px] text-muted-foreground">
          {m['instance.servers.players']({
            online: status.data.playersOnline,
            max: status.data.playersMax,
          })}
        </span>
      )}
      {online && status.data.version && (
        <Badge variant="outline" className="shrink-0 font-mono">
          {status.data.version}
        </Badge>
      )}

      <Button
        size="sm"
        variant="ghost"
        data-icon="inline-start"
        className="shrink-0"
        disabled={playing || !joinable}
        title={joinable ? undefined : m['instance.quick_play.unsupported']()}
        onClick={onPlay}
      >
        {playing ? <Spinner /> : <PlayIcon weight="fill" />}
        {m['app.action.join']()}
      </Button>

      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="ghost"
              size="icon-sm"
              className="shrink-0"
              aria-label={m['app.action.more']()}
            >
              <DotsThreeIcon weight="bold" className="size-4" />
            </Button>
          }
        />
        <DropdownMenuContent align="end" className="w-52">
          <DropdownMenuItem onClick={onEdit}>
            <PencilSimpleIcon />
            {m['app.action.edit']()}
          </DropdownMenuItem>
          <DropdownMenuItem
            closeOnClick={false}
            onClick={() =>
              void copy.run(() => navigator.clipboard.writeText(server.address))
            }
          >
            {copied ? <CheckIcon aria-hidden /> : <CopyIcon aria-hidden />}
            <span aria-hidden={copied}>
              {copied
                ? m['app.status.copied']()
                : m['instance.servers.copy_address']()}
            </span>
            <span role="status" className="sr-only">
              {copied ? m['app.status.copied']() : ''}
            </span>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onClick={onRemove}>
            <TrashIcon />
            {m['app.action.remove']()}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
