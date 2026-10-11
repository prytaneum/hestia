import {
  FolderOpenIcon,
  GlobeIcon,
  PlayIcon,
  PlusIcon,
  UploadSimpleIcon,
  WarningCircleIcon,
} from '@phosphor-icons/react';
import { useMutation, useQueries, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';
import type { ContentKind } from '@/api';
import { errorMessage, system } from '@/api';
import { DetailHero } from '@/components/detail-hero';
import { Empty } from '@/components/empty';
import { entryIcon } from '@/components/icons';
import { Stat, TabCount } from '@/components/page';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { StatusDot } from '@/components/ui/status-dot';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  ContentInstallDialog,
  instanceTarget,
  useEntryInstall,
} from '@/features/content/install';
import { SessionList, WorldRow } from '@/features/instances/components';
import {
  ExportInstanceDialog,
  useLaunchDialog,
} from '@/features/instances/dialogs';
import {
  InstanceLogsTab,
  InstanceServersTab,
  InstanceSettingsTab,
} from '@/features/instances/tabs';
import { anchor, useAutoTour } from '@/features/onboarding';
import {
  EntryIconMenu,
  EntryRunControl,
} from '@/features/shared/entry/components';
import {
  type LiveResources,
  ResourceCards,
} from '@/features/shared/entry/components/resource-panel';
import {
  ContentSection,
  ModpackCard,
  SideCard,
  StatCard,
} from '@/features/shared/entry/detail';
import { agoLabel, bytes, memGb, uptime } from '@/lib/format';
import { supportsQuickPlay } from '@/lib/quick-play';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import {
  instanceMutations,
  instanceQueries,
  useInstance,
} from '@/queries/instance';
import { useProcessMetrics } from '@/queries/metrics';

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
  const config = useQuery(instanceQueries.config(id));
  const worlds = useQuery(instanceQueries.worlds(id));
  const install = useEntryInstall(instance.name);
  const [exporting, setExporting] = useState(false);
  const [focus, setFocus] = useState<string | null>(null);
  const [logSession, setLogSession] = useState<string | null>(null);
  const { launch, isLaunching } = useLaunchDialog();
  const stop = useMutation(instanceMutations.stop(id));
  const accepts = instance.accepts;
  // Shared with the content tab's own per-kind queries (cached), just for the
  // headline count.
  const contentLists = useQueries({
    queries: accepts.map((k) => instanceQueries.content(id, k)),
  });
  const contentCount = contentLists.reduce(
    (n, q) => n + (q.data?.items.length ?? 0),
    0,
  );

  const sessions = runningSessions(instance);
  const running = sessions.length > 0;
  const liveSession =
    sessions.find((s) => s.id === focus) ?? sessions[sessions.length - 1];
  const metrics = useProcessMetrics(liveSession?.id ?? null);

  const memoryLimitGb = (() => {
    const value = config.data?.find((e) => e.key === 'memory')?.value;
    return value ? memGb(value) : 4;
  })();

  const live: LiveResources = {
    running,
    memoryLimitGb,
    diskBytes: info.data?.diskBytes ?? 0,
    series: metrics.series.map((s) => ({
      cpu: s.cpuPct,
      mem: s.memBytes / (1024 * 1024),
    })),
  };

  const openFolder = () => {
    if (info.data)
      system.openPath(info.data.entryDir).catch((error: Error) => {
        toast.error(errorMessage(error));
      });
  };

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
                {m['entry.sessions_running']({ count: sessions.length })}
              </Badge>
            )}
          </>
        }
        actions={
          <span className="flex items-center gap-2" {...anchor('entry-run')}>
            <Button
              variant="outline"
              size="icon"
              aria-label={m['app.action.open_folder']()}
              disabled={!info.data}
              onClick={openFolder}
            >
              <FolderOpenIcon className="size-4" />
            </Button>
            {running ? (
              <EntryRunControl
                name={instance.name}
                sessions={sessions}
                busy={stop.isPending}
                launching={isLaunching(id)}
                onNewSession={() => launch(instance, { newSession: true })}
                onStop={(session) => stop.mutate({ session })}
              />
            ) : (
              <Button
                data-icon="inline-start"
                disabled={isLaunching(id)}
                className="bg-ember text-ember-foreground hover:bg-ember/90"
                onClick={() => launch(instance)}
              >
                {isLaunching(id) ? <Spinner /> : <PlayIcon weight="fill" />}
                {m['app.action.play']()}
              </Button>
            )}
          </span>
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
              {running && (
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
                running={running}
              />
              <SideCard title={m['entry.quick_actions']()}>
                <div className="flex flex-col gap-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="justify-start"
                    data-icon="inline-start"
                    disabled={!info.data}
                    onClick={openFolder}
                  >
                    <FolderOpenIcon />
                    {m['app.action.open_folder']()}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="justify-start"
                    data-icon="inline-start"
                    onClick={() => setExporting(true)}
                  >
                    <UploadSimpleIcon />
                    {m['app.action.export']()}
                  </Button>
                </div>
              </SideCard>
            </div>
          </div>
        </TabsContent>

        <TabsContent
          {...install.getRootProps()}
          value="content"
          className="flex flex-col p-5"
        >
          <ContentSection
            entry={{
              kind: 'instance',
              id,
              flavor: instance.flavor,
              gameVersion: instance.gameVersion,
            }}
            kinds={accepts}
            kind={contentKind}
            onKindChange={onContentKindChange}
            action={
              <Button
                size="sm"
                variant="outline"
                data-icon="inline-start"
                onClick={install.start}
              >
                <PlusIcon weight="bold" />
                {m['content.add']()}
              </Button>
            }
          />
        </TabsContent>

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
                  playing={isLaunching(id, world.folder)}
                  disabledReason={
                    joinable
                      ? undefined
                      : m['instance.quick_play.unsupported']()
                  }
                  onPlay={() =>
                    launch(instance, {
                      quickPlay: { kind: 'world', target: world.folder },
                      newSession: running,
                    })
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
      <ExportInstanceDialog
        id={id}
        name={instance.name}
        open={exporting}
        onOpenChange={setExporting}
      />
    </div>
  );
}
