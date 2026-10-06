import {
  CloudIcon,
  GlobeHemisphereWestIcon,
  HardDrivesIcon,
} from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';

import type { InstanceInfo, RecentPlay } from '@/api';
import {
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';
import { pngSource, Thumbnail } from '@/components/ui/thumbnail';
import { useLaunchDialog } from '@/features/instances/dialogs';
import { agoLabel } from '@/lib/format';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { instanceQueries, useInstances } from '@/queries/instance';

const GLYPHS = {
  world: GlobeHemisphereWestIcon,
  server: HardDrivesIcon,
  realm: CloudIcon,
} as const;

/**
 * The worlds, servers and Realms joined lately, across every instance, as a
 * group of the quick-play menu — picking one launches the owning instance
 * straight back into that target. Renders nothing until something has been
 * played.
 */
export function RecentPlaysGroup() {
  const { signedIn } = useAccounts();
  const instances = useInstances();
  const recent = useQuery(instanceQueries.recent());
  const { launch } = useLaunchDialog();

  const plays = (recent.data ?? []).flatMap((play) => {
    const instance = instances.data?.find((i) => i.id === play.instance);
    return instance ? [{ play, instance }] : [];
  });
  if (plays.length === 0) return null;

  return (
    <>
      <DropdownMenuGroup>
        <DropdownMenuLabel>{m['app.playbar.recent']()}</DropdownMenuLabel>
        {plays.map(({ play, instance }) => (
          <DropdownMenuItem
            key={`${play.instance}:${play.target.kind}:${play.target.target}`}
            disabled={!signedIn}
            title={signedIn ? undefined : m['account.sign_in_to_play']()}
            onClick={() => launch(instance, launchOptions(play, instance))}
          >
            <Thumbnail
              src={pngSource(play.icon)}
              icon={GLYPHS[play.target.kind]}
              size="sm"
            />
            <span className="min-w-0 flex-1 leading-tight">
              <span className="block truncate">{play.name}</span>
              <span className="block truncate text-[11px] text-muted-foreground">
                {m['app.playbar.recent_play']({
                  instance: play.instanceName,
                  when: agoLabel(play.playedUnix),
                })}
              </span>
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
    </>
  );
}

/** A running instance is joined in a session of its own, as a world row does. */
function launchOptions(play: RecentPlay, instance: InstanceInfo) {
  return {
    quickPlay: play.target,
    newSession: runningSessions(instance).length > 0,
  };
}
