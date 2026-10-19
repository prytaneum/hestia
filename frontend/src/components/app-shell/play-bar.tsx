import {
  CaretUpDownIcon,
  CloudIcon,
  GlobeHemisphereWestIcon,
  HardDrivesIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useState } from 'react';

import type { RecentPlay } from '@/api';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { pngSource, Thumbnail } from '@/components/ui/thumbnail';
import { LaunchControl } from '@/features/instances/components/launch-control';
import { useLaunch } from '@/features/instances/hooks';
import { anchor } from '@/features/onboarding';
import { EntryRunControl } from '@/features/shared/entry/components';
import { agoLabel } from '@/lib/format';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import {
  instanceMutations,
  instanceQueries,
  useInstances,
} from '@/queries/instance';

const GLYPHS = {
  world: GlobeHemisphereWestIcon,
  server: HardDrivesIcon,
  realm: CloudIcon,
} as const;

const KINDS = {
  world: m['app.playbar.kind.world'],
  server: m['app.playbar.kind.server'],
  realm: m['app.playbar.kind.realm'],
} as const;

const keyOf = (play: RecentPlay) =>
  `${play.instance}:${play.target.kind}:${play.target.target}`;

/**
 * The quick-play strip along the bottom of the library: the world, server or
 * Realm joined most recently, and a button that launches its instance straight
 * back into it. The dropdown switches to another recent target; the button
 * gives way to the run control once a session is up. Absent until something
 * has been played — there is nothing to go back to.
 */
export function PlayBar() {
  const { signedIn } = useAccounts();
  const instances = useInstances();
  const recent = useQuery(instanceQueries.recent());
  const { launch, isLaunching } = useLaunch();
  const stop = useMutation(instanceMutations.stopAny());
  const [selKey, setSelKey] = useState<string | null>(null);

  const plays = (recent.data ?? []).flatMap((play) => {
    const instance = instances.data?.find((i) => i.id === play.instance);
    return instance ? [{ play, instance }] : [];
  });
  const sel = plays.find(({ play }) => keyOf(play) === selKey) ?? plays[0];
  if (!sel) return null;

  const { play, instance } = sel;
  const sessions = runningSessions(instance);
  const launching = isLaunching(instance.id);
  const stopping = stop.isPending && stop.variables?.id === instance.id;

  return (
    <div
      className="flex h-[76px] items-center gap-3 border-t border-border bg-sidebar px-4"
      {...anchor('play-bar')}
    >
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <button
              type="button"
              className="-ml-2 flex h-14 w-72 items-center gap-3 px-2 text-left transition-colors outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset aria-expanded:bg-muted"
            >
              <Thumbnail
                src={pngSource(play.icon)}
                fallback={instance.iconUrl}
                icon={GLYPHS[play.target.kind]}
                size="xl"
              />
              <span className="min-w-0 flex-1 leading-tight">
                <span className="block truncate text-base font-medium">
                  {play.name}
                </span>
                <span className="block truncate text-xs text-muted-foreground">
                  {m['app.playbar.recent_play']({
                    kind: KINDS[play.target.kind](),
                    instance: instance.name,
                  })}
                </span>
              </span>
              <CaretUpDownIcon className="size-4 shrink-0 text-muted-foreground" />
            </button>
          }
        />
        <DropdownMenuContent side="top" align="start" className="w-72">
          <DropdownMenuGroup>
            <DropdownMenuLabel>{m['app.playbar.recent']()}</DropdownMenuLabel>
            {plays.map((entry) => (
              <RecentPlayItem
                key={keyOf(entry.play)}
                play={entry.play}
                instanceIcon={entry.instance.iconUrl}
                onSelect={() => setSelKey(keyOf(entry.play))}
              />
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <div className="ml-auto hidden items-center gap-1.5 sm:flex">
        <Badge variant="secondary" className="uppercase">
          {instance.flavor}
        </Badge>
        <Badge variant="outline" className="font-mono">
          {instance.gameVersion}
        </Badge>
      </div>

      <Button
        variant="ghost"
        size="sm"
        nativeButton={false}
        render={<Link to="/instances/$id" params={{ id: instance.id }} />}
      >
        {m['app.action.manage']()}
      </Button>

      {sessions.length > 0 ? (
        <EntryRunControl
          name={instance.name}
          sessions={sessions}
          size="sm"
          busy={stopping}
          launching={launching}
          onNewSession={() =>
            launch(instance.id, { quickPlay: play.target, newSession: true })
          }
          onStop={(session) => stop.mutate({ id: instance.id, session })}
        />
      ) : (
        <LaunchControl
          instanceId={instance.id}
          size="bar"
          label="join"
          quickPlay={play.target}
          disabled={!signedIn || stopping}
          disabledReason={signedIn ? undefined : m['account.sign_in_to_play']()}
        />
      )}
    </div>
  );
}

function RecentPlayItem({
  play,
  instanceIcon,
  onSelect,
}: {
  play: RecentPlay;
  instanceIcon?: string;
  onSelect: () => void;
}) {
  return (
    <DropdownMenuItem onClick={onSelect}>
      <Thumbnail
        src={pngSource(play.icon)}
        fallback={instanceIcon}
        icon={GLYPHS[play.target.kind]}
        size="sm"
      />
      <span className="min-w-0 flex-1 leading-tight">
        <span className="block truncate">{play.name}</span>
        <span className="block truncate text-[11px] text-muted-foreground">
          {m['app.playbar.recent_play']({
            kind: KINDS[play.target.kind](),
            instance: play.instanceName,
          })}
        </span>
      </span>
      <span className="shrink-0 self-start text-[11px] text-muted-foreground">
        {agoLabel(play.playedUnix)}
      </span>
    </DropdownMenuItem>
  );
}
