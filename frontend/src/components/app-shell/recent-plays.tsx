import {
  CloudIcon,
  GlobeHemisphereWestIcon,
  HardDrivesIcon,
} from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';

import type { InstanceInfo, RecentPlay } from '@/api';
import { Spinner } from '@/components/ui/spinner';
import { pngSource, Thumbnail } from '@/components/ui/thumbnail';
import { useLaunchDialog } from '@/features/instances/dialogs';
import { agoLabel } from '@/lib/format';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { instanceQueries, useInstances } from '@/queries/instance';

const msg = m as unknown as Record<string, () => string>;

const GLYPHS = {
  world: GlobeHemisphereWestIcon,
  server: HardDrivesIcon,
  realm: CloudIcon,
} as const;

/**
 * The worlds, servers and Realms joined lately, across every instance — one
 * click launches the owning instance straight back into that target. Renders
 * nothing until something has been played.
 */
export function RecentPlays() {
  const { signedIn } = useAccounts();
  const instances = useInstances();
  const recent = useQuery(instanceQueries.recent());
  const { launch, isLaunching } = useLaunchDialog();

  const plays = (recent.data ?? []).flatMap((play) => {
    const instance = instances.data?.find((i) => i.id === play.instance);
    return instance ? [{ play, instance }] : [];
  });
  if (plays.length === 0) return null;

  return (
    <div className="hidden min-w-0 flex-col gap-1 md:flex">
      <span className="text-[11px] tracking-wide text-muted-foreground uppercase">
        {m['app.playbar.recent']()}
      </span>
      <div className="flex items-center gap-1.5">
        {plays.map(({ play, instance }) => (
          <RecentPlayButton
            key={`${play.instance}:${play.target.kind}:${play.target.target}`}
            play={play}
            disabled={!signedIn}
            launching={isLaunching(instance.id, play.target.target)}
            onPlay={() => launch(instance, launchOptions(play, instance))}
          />
        ))}
      </div>
    </div>
  );
}

/** A running instance is joined in a session of its own, as a world row does. */
function launchOptions(play: RecentPlay, instance: InstanceInfo) {
  return {
    quickPlay: play.target,
    newSession: runningSessions(instance).length > 0,
  };
}

function RecentPlayButton({
  play,
  disabled,
  launching,
  onPlay,
}: {
  play: RecentPlay;
  disabled: boolean;
  launching: boolean;
  onPlay: () => void;
}) {
  const label = m['app.playbar.recent_play']({
    name: play.name,
    instance: play.instanceName,
    mode: msg[`domain.gamemode.${play.gameMode}`](),
    when: agoLabel(play.playedUnix),
  });
  return (
    <button
      type="button"
      disabled={disabled || launching}
      title={disabled ? m['account.sign_in_to_play']() : label}
      aria-label={label}
      onClick={onPlay}
      className="transition-opacity outline-none hover:opacity-80 focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
    >
      <Thumbnail
        src={pngSource(play.icon)}
        icon={GLYPHS[play.target.kind]}
        size="md"
      >
        {launching && (
          <span className="absolute inset-0 grid place-items-center bg-background/70">
            <Spinner />
          </span>
        )}
      </Thumbnail>
    </button>
  );
}
