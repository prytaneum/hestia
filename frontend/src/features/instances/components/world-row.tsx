import type { WorldInfo } from '@/api';
import { Badge } from '@/components/ui/badge';
import { WorldIcon } from '@/features/shared/entry/components';
import { agoLabel, bytes } from '@/lib/format';
import { m } from '@/paraglide/messages.js';

const msg = m as unknown as Record<string, () => string>;

/**
 * One save world, as its own `level.dat` describes it: the player's name for it
 * over the folder the game reads, with the flags that change how it plays. A
 * world we could not read shows its folder and says so, rather than presenting
 * defaults as facts.
 *
 * `play` adds the join-on-start action; without it the row is the plain
 * listing the datapack picker renders.
 */
export function WorldRow({
  world,
  play,
}: {
  world: WorldInfo;
  /** The join control; absent leaves the plain listing. */
  play?: React.ReactNode;
}) {
  return (
    <div className="group flex items-center gap-3 px-3 py-2.5">
      <WorldIcon world={world} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm">{world.name}</span>
          {world.hardcore && (
            <Badge variant="outline" className="shrink-0">
              {m['instance.world.hardcore']()}
            </Badge>
          )}
          {world.cheats && (
            <Badge variant="outline" className="shrink-0">
              {m['instance.world.cheats']()}
            </Badge>
          )}
        </div>
        <div className="truncate font-mono text-[11px] text-muted-foreground">
          {world.read
            ? `${world.folder} · ${msg[`domain.gamemode.${world.gameMode}`]()} · ${msg[`domain.difficulty.${world.difficulty}`]()}`
            : m['instance.world.unreadable']()}
        </div>
      </div>
      <div className="shrink-0 text-right text-[11px] text-muted-foreground">
        {world.lastPlayedUnix != null && (
          <div>{agoLabel(world.lastPlayedUnix)}</div>
        )}
        <div className="font-mono">{bytes(world.sizeBytes)}</div>
      </div>
      {world.version && (
        <Badge variant="outline" className="shrink-0 font-mono">
          {world.version}
        </Badge>
      )}
      {play && <div className="shrink-0">{play}</div>}
    </div>
  );
}
