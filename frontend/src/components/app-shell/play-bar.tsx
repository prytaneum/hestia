import { CaretUpDownIcon, PlayIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useState } from 'react';

import type { InstanceInfo } from '@/api';
import { RecentPlaysGroup } from '@/components/app-shell/recent-plays';
import { entryIcon } from '@/components/icons';
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
import { Thumbnail } from '@/components/ui/thumbnail';
import { useLaunchDialog } from '@/features/instances/dialogs';
import { anchor } from '@/features/onboarding';
import { EntryRunControl } from '@/features/shared/entry/components';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { instanceMutations, useInstances } from '@/queries/instance';

/**
 * The always-present quick-play strip along the bottom of the library. The
 * instance is chosen from a dropdown; the button launches it, and gives way to
 * the run control once a session is up. The dropdown leads with the targets
 * played lately, each a launch straight back into it.
 */
export function PlayBar() {
  const { signedIn } = useAccounts();
  const instances = useInstances();
  const { launch, isLaunching } = useLaunchDialog();
  const stop = useMutation(instanceMutations.stopAny());

  const list = instances.data ?? [];
  const [selId, setSelId] = useState<string | null>(null);
  const sel = list.find((i) => i.id === selId) ?? list[0];

  const Icon = entryIcon('instance');
  const sessions = sel ? runningSessions(sel) : [];
  const launching = sel ? isLaunching(sel.id) : false;
  const stopping = stop.isPending && stop.variables?.id === sel?.id;
  const busy = launching || stopping;

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
              disabled={list.length === 0}
              className="-ml-2 flex h-14 w-72 items-center gap-3 px-2 text-left transition-colors outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset aria-expanded:bg-muted disabled:opacity-50"
            >
              <Thumbnail src={sel?.iconUrl} icon={Icon} size="xl" />
              <span className="min-w-0 flex-1 leading-tight">
                <span className="block text-[11px] tracking-wide text-muted-foreground uppercase">
                  {m['app.playbar.quick_play']()}
                </span>
                <span className="block truncate text-base font-medium">
                  {sel?.name ?? '—'}
                </span>
              </span>
              <CaretUpDownIcon className="size-4 shrink-0 text-muted-foreground" />
            </button>
          }
        />
        <DropdownMenuContent side="top" align="start" className="w-72">
          <RecentPlaysGroup />
          <DropdownMenuGroup>
            <DropdownMenuLabel>
              {m['app.playbar.all_instances']()}
            </DropdownMenuLabel>
            {list.map((i) => (
              <InstanceItem
                key={i.id}
                instance={i}
                onSelect={() => setSelId(i.id)}
              />
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <div className="ml-auto hidden items-center gap-1.5 sm:flex">
        {sel && (
          <>
            <Badge variant="secondary" className="uppercase">
              {sel.flavor}
            </Badge>
            <Badge variant="outline" className="font-mono">
              {sel.gameVersion}
            </Badge>
          </>
        )}
      </div>

      {sel && (
        <Button
          variant="ghost"
          size="sm"
          nativeButton={false}
          render={<Link to="/instances/$id" params={{ id: sel.id }} />}
        >
          {m['app.action.manage']()}
        </Button>
      )}

      {sel && sessions.length > 0 ? (
        <EntryRunControl
          name={sel.name}
          sessions={sessions}
          size="sm"
          busy={stopping}
          launching={launching}
          onNewSession={() => launch(sel, { newSession: true })}
          onStop={(session) => stop.mutate({ id: sel.id, session })}
        />
      ) : (
        <Button
          data-icon="inline-start"
          disabled={!signedIn || !sel || busy}
          title={signedIn ? undefined : m['account.sign_in_to_play']()}
          onClick={() => sel && launch(sel)}
          className="bg-ember text-ember-foreground hover:bg-ember/90"
        >
          {busy ? <Spinner /> : <PlayIcon weight="fill" />}
          {m['app.action.play']()}
        </Button>
      )}
    </div>
  );
}

function InstanceItem({
  instance,
  onSelect,
}: {
  instance: InstanceInfo;
  onSelect: () => void;
}) {
  const sessions = runningSessions(instance).length;
  return (
    <DropdownMenuItem onClick={onSelect}>
      <span className="min-w-0 flex-1 truncate">{instance.name}</span>
      {sessions > 0 ? (
        <>
          {sessions > 1 && (
            <span
              className="font-mono text-[10px] text-muted-foreground"
              title={m['entry.sessions_running']({ count: sessions })}
            >
              ×{sessions}
            </span>
          )}
          <StatusDot tone="on" />
        </>
      ) : (
        <span className="font-mono text-[10px] text-muted-foreground">
          {instance.gameVersion}
        </span>
      )}
    </DropdownMenuItem>
  );
}
