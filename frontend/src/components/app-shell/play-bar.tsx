import {
  CaretUpDownIcon,
  CloudIcon,
  GlobeHemisphereWestIcon,
  HardDrivesIcon,
  PlayIcon,
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
import { Spinner } from '@/components/ui/spinner';
import { StatusDot } from '@/components/ui/status-dot';
import { pngSource, Thumbnail } from '@/components/ui/thumbnail';
import { useLaunchDialog } from '@/features/instances/dialogs';
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

const msg = m as unknown as Record<string, () => string>;

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
  const { launch, isLaunching } = useLaunchDialog();
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
  const launching = isLaunching(instance.id, play.target.target);
  const stopping = stop.isPending && stop.variables?.id === instance.id;
  const busy = launching || stopping;
  const join = (newSession: boolean) =>
    launch(instance, { quickPlay: play.target, newSession });

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
                <span className="block truncate text-[11px] tracking-wide text-muted-foreground uppercase">
                  {m['app.playbar.target']({
                    kind: KINDS[play.target.kind](),
                    mode: msg[`domain.gamemode.${play.gameMode}`](),
                  })}
                </span>
                <span className="block truncate text-base font-medium">
                  {play.name}
                </span>
                <span className="block truncate text-[11px] text-muted-foreground">
                  {m['app.playbar.played']({
                    instance: instance.name,
                    when: agoLabel(play.playedUnix),
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
                running={runningSessions(entry.instance).length > 0}
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
          onNewSession={() => join(true)}
          onStop={(session) => stop.mutate({ id: instance.id, session })}
        />
      ) : (
        <Button
          data-icon="inline-start"
          disabled={!signedIn || busy}
          title={signedIn ? undefined : m['account.sign_in_to_play']()}
          onClick={() => join(false)}
          className="bg-ember text-ember-foreground hover:bg-ember/90"
        >
          {busy ? <Spinner /> : <PlayIcon weight="fill" />}
          {m['app.action.join']()}
        </Button>
      )}
    </div>
  );
}

function RecentPlayItem({
  play,
  instanceIcon,
  running,
  onSelect,
}: {
  play: RecentPlay;
  instanceIcon?: string;
  running: boolean;
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
            when: agoLabel(play.playedUnix),
          })}
        </span>
      </span>
      {running && <StatusDot tone="on" />}
    </DropdownMenuItem>
  );
}
