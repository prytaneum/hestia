import { GlobeIcon, WarningCircleIcon } from '@phosphor-icons/react';
import { useMutation, useQueries, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import type { ContentKind } from '@/api';
import { DetailHero } from '@/components/detail-hero';
import { Empty } from '@/components/empty';
import { entryIcon } from '@/components/icons';
import { Stat, TabCount } from '@/components/page';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { StatusDot } from '@/components/ui/status-dot';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  ContentInstallDialog,
  instanceTarget,
  useEntryInstall,
} from '@/features/content/install';
import { SessionList, WorldRow } from '@/features/instances/components';
import { LaunchControl } from '@/features/instances/components/launch-control';
import { useLaunch } from '@/features/instances/hooks';
import {
  InstanceLogsTab,
  InstanceServersTab,
  InstanceSettingsTab,
} from '@/features/instances/tabs';
import { anchor, useAutoTour } from '@/features/onboarding';
import {
  busyReason,
  EntryActionsMenu,
  EntryIconMenu,
  EntryJobProgress,
  EntryRunControl,
} from '@/features/shared/entry/components';
import { ResourceCards } from '@/features/shared/entry/components/resource-panel';
import {
  EntryContentTab,
  ModpackCard,
  SideCard,
  StatCard,
  useLiveResources,
} from '@/features/shared/entry/detail';
import { agoLabel, bytes, uptime } from '@/lib/format';
import { supportsQuickPlay } from '@/lib/quick-play';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { useSessionsShown } from '@/queries/config';
import { entryMutations, entryQueries } from '@/queries/entry';
import { entryContentQueries } from '@/queries/entry-content';
import { instanceQueries, useInstance } from '@/queries/instance';
import { useBusyJob } from '@/queries/jobs';

export type InstanceTab =
  | 'overview'
  | 'content'
  | 'worlds'
  | 'servers'
  | 'logs'
  | 'settings';

/** The record as the hook serves it — joined with its icon URL. */
type Instance = NonNullable<ReturnType<typeof useInstance>['data']>;

interface DetailProps {
  tab: InstanceTab;
  onTabChange: (tab: InstanceTab) => void;
  contentKind?: ContentKind;
  onContentKindChange: (kind?: ContentKind) => void;
}

/**
 * Resolves the entry before anything reads from it: every per-instance query
 * below is keyed by an id the daemon may no longer have (removed here, from
 * the CLI, or by the tray), so the body only mounts while the list still
 * carries it — an id that is gone asks the daemon nothing.
 */
export function InstanceDetailPage({
  id,
  ...props
}: DetailProps & { id: string }) {
  const query = useInstance(id);

  if (query.isPending) {
    return (
      <div className="space-y-4 p-6">
        <Bone className="h-8 w-64" />
        <Bone className="h-40" />
      </div>
    );
  }

  if (!query.data) {
    return (
      <div className="p-6">
        <Empty icon={WarningCircleIcon}>{m['instance.missing']()}</Empty>
      </div>
    );
  }

  return <InstanceDetail instance={query.data} {...props} />;
}

function InstanceDetail({
  instance,
  tab,
  onTabChange,
  contentKind,
  onContentKindChange,
}: DetailProps & { instance: Instance }) {
  const id = instance.id;
  const info = useQuery(instanceQueries.info(id));
  useAutoTour('instance', Boolean(info.data));
  const config = useQuery(entryQueries.config('instance', id));
  const worlds = useQuery(instanceQueries.worlds(id));
  const install = useEntryInstall(instance.name);
  const [focus, setFocus] = useState<string | null>(null);
  const [logSession, setLogSession] = useState<string | null>(null);
  const { launch, restart, isLaunching } = useLaunch();
  const stop = useMutation(entryMutations.stop('instance', id));
  const accepts = instance.accepts;
  // Shared with the content tab's own per-kind queries (cached), just for the
  // headline count.
  const contentLists = useQueries({
    queries: accepts.map((k) => entryContentQueries.list('instance', id, k)),
  });
  const contentCount = contentLists.reduce(
    (n, q) => n + (q.data?.items.length ?? 0),
    0,
  );

  const job = useBusyJob('instance', id);
  const sessions = runningSessions(instance);
  const running = sessions.length > 0;
  const perSession = useSessionsShown(sessions.length);
  const liveSession =
    sessions.find((s) => s.id === focus) ?? sessions[sessions.length - 1];
  const { live, memoryLimitGb } = useLiveResources({
    running,
    config: config.data,
    diskBytes: info.data?.diskBytes,
    processId: liveSession?.id ?? null,
  });

  const worldList = worlds.data ?? [];
  const joinable = supportsQuickPlay(instance.gameVersion);

  return (
    <div className="flex h-full flex-col">
      <DetailHero
        parentLabel={m['app.nav.library']()}
        parentTo="/instances"
        icon={entryIcon('instance')}
        iconUrl={instance.iconUrl}
        iconAction={<EntryIconMenu id={id} />}
        name={instance.name}
        busy={job && <EntryJobProgress job={job} name={instance.name} />}
        badges={
          <>
            <Badge variant="secondary" className="uppercase">
              {instance.flavor}
            </Badge>
            <Badge variant="outline" className="font-mono">
              {instance.gameVersion}
            </Badge>
            {running && (
              <Badge variant="secondary" className="gap-1.5">
                <StatusDot tone="on" />
                {perSession
                  ? m['entry.sessions_running']({ count: sessions.length })
                  : m['app.status.running']()}
              </Badge>
            )}
          </>
        }
        actions={
          <>
            <span className="flex items-center gap-2" {...anchor('entry-run')}>
              {running ? (
                <EntryRunControl
                  name={instance.name}
                  sessions={sessions}
                  busy={stop.isPending}
                  launching={isLaunching(id)}
                  onNewSession={() => launch(id, { newSession: true })}
                  onRestart={(session) => restart(id, session)}
                  onStop={(session) => stop.mutate({ session })}
                />
              ) : (
                <LaunchControl
                  instanceId={id}
                  disabled={job !== null}
                  disabledReason={job ? busyReason(job) : undefined}
                />
              )}
            </span>
            <EntryActionsMenu
              entry={{ kind: 'instance', id, name: instance.name, running }}
            />
          </>
        }
      />

      <Tabs
        value={tab}
        onValueChange={(value) => onTabChange(value as InstanceTab)}
        className="min-h-0 flex-1 gap-0 p-0"
      >
        <TabsList variant="line" className="h-auto gap-6 px-5">
          <TabsTrigger value="overview">
            {m['app.label.overview']()}
          </TabsTrigger>
          <TabsTrigger value="content" {...anchor('instance-content')}>
            {m['app.label.content']()}
            <TabCount n={contentCount} />
          </TabsTrigger>
          <TabsTrigger value="worlds" {...anchor('instance-worlds')}>
            {m['app.label.worlds']()}
            <TabCount n={worldList.length} />
          </TabsTrigger>
          <TabsTrigger value="servers">{m['app.label.servers']()}</TabsTrigger>
          <TabsTrigger value="logs">{m['app.label.logs']()}</TabsTrigger>
          <TabsTrigger value="settings">
            {m['app.label.settings']()}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="overview" keepMounted className="flex flex-col p-5">
          <div className="grid flex-1 gap-6 lg:grid-cols-[1fr_260px]">
            <div className="flex flex-col gap-5">
              <p className="max-w-2xl text-sm leading-relaxed text-foreground/90">
                {m['entry.overview_summary']({
                  flavor: instance.flavor,
                  version: instance.gameVersion,
                  mods: contentCount,
                  worlds: worldList.length,
                })}
              </p>
              <div className="grid grid-cols-3 gap-3">
                <StatCard
                  value={contentCount}
                  label={m['app.label.content']()}
                />
                <StatCard
                  value={worldList.length}
                  label={m['app.label.worlds']()}
                />
                <StatCard
                  value={memoryLimitGb ? `${memoryLimitGb}G` : '—'}
                  label={m['app.label.memory']()}
                />
              </div>
              <ResourceCards live={live} />
              {running && perSession && (
                <section className="space-y-2">
                  <h2 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                    {m['app.label.sessions']()}
                  </h2>
                  <SessionList
                    sessions={sessions}
                    focused={liveSession?.id}
                    onFocus={setFocus}
                    onLogs={(session) => {
                      setLogSession(session);
                      onTabChange('logs');
                    }}
                    onStop={(session) => stop.mutate({ session })}
                    stopping={stop.isPending}
                  />
                </section>
              )}
            </div>

            <div className="space-y-4">
              <SideCard title={m['app.label.details']()}>
                <div className="divide-y divide-border">
                  <Stat
                    label={m['app.label.loader']()}
                    value={instance.flavor}
                  />
                  <Stat
                    label={m['app.label.version']()}
                    value={instance.gameVersion}
                  />
                  <Stat
                    label={m['app.label.java']()}
                    value={instance.javaMajor}
                  />
                  <Stat
                    label={m['app.label.created']()}
                    value={agoLabel(instance.createdUnix)}
                  />
                  <Stat
                    label={m['app.label.last_played']()}
                    value={
                      info.data?.lastPlayedUnix
                        ? agoLabel(info.data.lastPlayedUnix)
                        : '—'
                    }
                  />
                  <Stat
                    label={m['app.label.playtime']()}
                    value={
                      info.data?.playtimeSeconds
                        ? uptime(info.data.playtimeSeconds)
                        : '—'
                    }
                  />
                  <Stat
                    label={m['app.label.disk']()}
                    value={
                      info.data?.diskBytes != null
                        ? bytes(info.data.diskBytes)
                        : '—'
                    }
                  />
                </div>
              </SideCard>
              <ModpackCard
                kind="instance"
                id={instance.id}
                name={instance.name}
                flavor={instance.flavor}
                gameVersion={instance.gameVersion}
                running={running}
              />
            </div>
          </div>
        </TabsContent>

        <EntryContentTab
          install={install}
          entry={{
            kind: 'instance',
            id,
            flavor: instance.flavor,
            gameVersion: instance.gameVersion,
          }}
          kinds={accepts}
          kind={contentKind}
          onKindChange={onContentKindChange}
        />

        <TabsContent value="worlds" className="flex flex-col p-5">
          {worlds.isPending ? (
            <div className="space-y-2">
              <Bone className="h-10" />
              <Bone className="h-10" />
            </div>
          ) : worldList.length === 0 ? (
            <Empty className="flex-1" icon={GlobeIcon}>
              {m['instance.no_worlds']()}
            </Empty>
          ) : (
            <div className="divide-y divide-border border border-border">
              {worldList.map((world) => (
                <WorldRow
                  key={world.folder}
                  world={world}
                  play={
                    <LaunchControl
                      instanceId={id}
                      size="sm"
                      quickPlay={{ kind: 'world', target: world.folder }}
                      newSession={running}
                      disabled={!joinable}
                      disabledReason={m['instance.quick_play.unsupported']()}
                    />
                  }
                />
              ))}
            </div>
          )}
        </TabsContent>

        <TabsContent value="servers" className="flex flex-col p-5">
          <InstanceServersTab instance={instance} />
        </TabsContent>

        <TabsContent value="logs" className="flex min-h-0 flex-col p-5">
          <InstanceLogsTab
            id={id}
            name={instance.name}
            sessions={sessions}
            session={logSession}
            onSessionChange={setLogSession}
          />
        </TabsContent>

        <TabsContent value="settings" className="p-5">
          <InstanceSettingsTab
            instance={instance}
            config={config.data}
            running={running}
          />
        </TabsContent>
      </Tabs>

      <ContentInstallDialog
        entry={instanceTarget(instance)}
        files={install.files}
        open={install.open}
        onOpenChange={install.onOpenChange}
      />
    </div>
  );
}
