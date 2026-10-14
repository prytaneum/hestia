import {
  ArrowClockwiseIcon,
  FolderOpenIcon,
  PlayIcon,
  PlusIcon,
  PowerIcon,
  WarningCircleIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { toast } from 'sonner';
import type { ContentKind } from '@/api';
import { errorMessage, type ServerInfo, system } from '@/api';
import { DetailHero } from '@/components/detail-hero';
import { Empty } from '@/components/empty';
import { entryIcon } from '@/components/icons';
import { Stat } from '@/components/page';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Spinner } from '@/components/ui/spinner';
import { StatusDot } from '@/components/ui/status-dot';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { WarningNotice } from '@/components/warning-notice';
import {
  ContentInstallDialog,
  serverTarget,
  useEntryInstall,
} from '@/features/content/install';
import { anchor, useAutoTour } from '@/features/onboarding';
import {
  ServerBackupsTab,
  ServerConsoleTab,
  ServerFilesTab,
  ServerPlayersTab,
  ServerSettingsTab,
} from '@/features/servers/tabs';
import {
  EntryIconMenu,
  EntryPinButton,
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
import { agoLabel, bytes, memGb } from '@/lib/format';
import { m } from '@/paraglide/messages.js';
import { useProcessMetrics } from '@/queries/metrics';
import { serverMutations, serverQueries, useServer } from '@/queries/server';

export type ServerTab =
  | 'overview'
  | 'console'
  | 'content'
  | 'players'
  | 'files'
  | 'backups'
  | 'settings';

function isRunning(server: ServerInfo): boolean {
  return server.process?.state === 'running';
}

/** The record as the hook serves it — joined with its icon URL. */
type Server = NonNullable<ReturnType<typeof useServer>['data']>;

interface DetailProps {
  tab: ServerTab;
  onTabChange: (tab: ServerTab) => void;
  contentKind?: ContentKind;
  onContentKindChange: (kind?: ContentKind) => void;
  filesPath: string;
  openFile?: string;
  onFilesNavigate: (path: string) => void;
  onOpenFile: (file?: string) => void;
}

/**
 * Resolves the entry before anything reads from it: every per-server query
 * below is keyed by an id the daemon may no longer have (removed here, from
 * the CLI, or by the tray), so the body only mounts while the list still
 * carries it — an id that is gone asks the daemon nothing.
 */
export function ServerDetailPage({
  id,
  ...props
}: DetailProps & { id: string }) {
  const query = useServer(id);

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
        <Empty icon={WarningCircleIcon}>{m['server.missing']()}</Empty>
      </div>
    );
  }

  return <ServerDetail server={query.data} {...props} />;
}

function ServerDetail({
  server,
  tab,
  onTabChange,
  contentKind,
  onContentKindChange,
  filesPath,
  openFile,
  onFilesNavigate,
  onOpenFile,
}: DetailProps & { server: Server }) {
  const id = server.id;
  const info = useQuery(serverQueries.info(id));
  useAutoTour('server', Boolean(info.data));
  const config = useQuery(serverQueries.config(id));
  const install = useEntryInstall(server.name);
  const start = useMutation(serverMutations.start(id));
  const stop = useMutation(serverMutations.stop(id));
  const restart = useMutation(serverMutations.restart(id));

  const running = isRunning(server);
  const ping = useQuery({ ...serverQueries.ping(id), enabled: running });
  const metrics = useProcessMetrics(server.process?.id ?? null);

  const memoryLimitGb = useMemo(() => {
    const value = config.data?.find((e) => e.key === 'memory')?.value;
    return value ? memGb(value) : 4;
  }, [config.data]);

  const statusTone = !server.ready ? 'warn' : running ? 'on' : 'off';
  const statusLabel = !server.ready
    ? m['app.status.preparing']()
    : running
      ? m['app.status.online']()
      : m['app.status.stopped']();

  const live: LiveResources = {
    running,
    memoryLimitGb,
    diskBytes: info.data?.diskBytes ?? 0,
    series: metrics.series.map((s) => ({
      cpu: s.cpuPct,
      mem: s.memBytes / (1024 * 1024),
    })),
  };

  return (
    <div className="flex h-full flex-col">
      <DetailHero
        parentLabel={m['app.nav.servers']()}
        parentTo="/servers"
        icon={entryIcon('server')}
        iconUrl={server.iconUrl}
        iconAction={<EntryIconMenu id={id} />}
        name={server.name}
        badges={
          <>
            <Badge variant="secondary" className="uppercase">
              {server.flavor}
            </Badge>
            <Badge variant="outline" className="font-mono">
              {server.gameVersion}
            </Badge>
            <Badge variant="secondary" className="gap-1.5">
              <StatusDot tone={statusTone} />
              {statusLabel}
            </Badge>
          </>
        }
        actions={
          <span className="flex items-center gap-2" {...anchor('entry-run')}>
            <EntryPinButton kind="server" id={id} />
            <Button
              variant="outline"
              size="icon"
              aria-label={m['app.action.open_folder']()}
              disabled={!info.data}
              onClick={() => {
                if (info.data)
                  system.openPath(info.data.entryDir).catch((error: Error) => {
                    toast.error(errorMessage(error));
                  });
              }}
            >
              <FolderOpenIcon className="size-4" />
            </Button>
            {running || restart.isPending ? (
              <>
                <ConfirmDialog
                  trigger={
                    <Button
                      variant="outline"
                      data-icon="inline-start"
                      disabled={stop.isPending || restart.isPending}
                    >
                      {restart.isPending ? (
                        <Spinner />
                      ) : (
                        <ArrowClockwiseIcon weight="bold" />
                      )}
                      {m['app.action.restart']()}
                    </Button>
                  }
                  title={m['entry.restart.title']({ name: server.name })}
                  description={m['entry.restart.server_description']()}
                  confirmLabel={m['app.action.restart']()}
                  onConfirm={() => restart.mutate()}
                />
                <ConfirmDialog
                  trigger={
                    <Button
                      variant="outline"
                      data-icon="inline-start"
                      disabled={stop.isPending || restart.isPending}
                    >
                      <PowerIcon weight="bold" />
                      {m['app.action.stop']()}
                    </Button>
                  }
                  title={m['entry.stop.title']({ name: server.name })}
                  description={m['entry.stop.server_description']()}
                  confirmLabel={m['app.action.stop']()}
                  onConfirm={() => stop.mutate()}
                />
              </>
            ) : (
              <Button
                disabled={!server.ready || start.isPending}
                data-icon="inline-start"
                className="bg-ember text-ember-foreground hover:bg-ember/90"
                onClick={() => start.mutate()}
              >
                <PlayIcon weight="fill" />
                {m['app.action.start']()}
              </Button>
            )}
          </span>
        }
      />

      <Tabs
        value={tab}
        onValueChange={(value) => onTabChange(value as ServerTab)}
        className="min-h-0 flex-1 gap-0 p-0"
      >
        <TabsList variant="line" className="h-auto gap-6 px-5">
          <TabsTrigger value="overview">
            {m['app.label.overview']()}
          </TabsTrigger>
          <TabsTrigger value="console" {...anchor('server-console')}>
            {m['app.label.console']()}
          </TabsTrigger>
          <TabsTrigger value="content">{m['app.label.content']()}</TabsTrigger>
          <TabsTrigger value="players">{m['app.label.players']()}</TabsTrigger>
          <TabsTrigger value="files">{m['server.files.title']()}</TabsTrigger>
          <TabsTrigger value="backups" {...anchor('server-backups')}>
            {m['app.label.backups']()}
          </TabsTrigger>
          <TabsTrigger value="settings">
            {m['app.label.settings']()}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="overview" keepMounted className="flex flex-col p-5">
          <div className="grid flex-1 gap-6 lg:grid-cols-[1fr_260px]">
            <div className="flex flex-col gap-5">
              <WarningNotice
                warnings={info.data?.warnings}
                className="w-full"
              />
              {ping.data?.motd && (
                <p className="max-w-2xl text-sm leading-relaxed text-foreground/90">
                  {ping.data.motd}
                </p>
              )}
              <div className="grid grid-cols-3 gap-3">
                <StatCard
                  value={
                    ping.data
                      ? `${ping.data.playersOnline}/${ping.data.playersMax}`
                      : '—'
                  }
                  label={m['app.label.players']()}
                />
                <StatCard
                  value={memoryLimitGb ? `${memoryLimitGb}G` : '—'}
                  label={m['app.label.memory']()}
                />
                <StatCard
                  value={
                    info.data?.diskBytes != null
                      ? bytes(info.data.diskBytes)
                      : '—'
                  }
                  label={m['app.label.disk']()}
                />
              </div>
              <ResourceCards live={live} />
            </div>

            <div className="space-y-4">
              <span className="block" {...anchor('server-details')}>
                <SideCard title={m['app.label.details']()}>
                  <div className="divide-y divide-border">
                    <Stat
                      label={m['app.label.address']()}
                      value={`localhost:${server.gamePort ?? '—'}`}
                    />
                    <Stat
                      label={m['app.label.loader']()}
                      value={server.flavor}
                    />
                    <Stat
                      label={m['app.label.version']()}
                      value={server.gameVersion}
                    />
                    <Stat
                      label={m['app.label.java']()}
                      value={server.javaMajor}
                    />
                    <Stat
                      label={m['app.label.created']()}
                      value={agoLabel(server.createdUnix)}
                    />
                  </div>
                </SideCard>
              </span>
              <ModpackCard
                kind="server"
                id={server.id}
                name={server.name}
                flavor={server.flavor}
                gameVersion={server.gameVersion}
                running={running}
              />
            </div>
          </div>
        </TabsContent>

        <TabsContent value="console" className="flex min-h-0 flex-col p-5">
          <ServerConsoleTab id={id} running={running} name={server.name} />
        </TabsContent>

        <TabsContent
          {...install.getRootProps()}
          value="content"
          className="flex flex-col p-5"
        >
          <ContentSection
            entry={{
              kind: 'server',
              id,
              flavor: server.flavor,
              gameVersion: server.gameVersion,
            }}
            kinds={server.accepts ?? []}
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

        <TabsContent value="players" className="flex min-h-0 flex-col p-5">
          <ServerPlayersTab id={id} running={running} />
        </TabsContent>

        <TabsContent value="files" className="flex min-h-0 flex-col p-5">
          <ServerFilesTab
            id={id}
            running={running}
            path={filesPath}
            file={openFile}
            onNavigate={onFilesNavigate}
            onOpenFile={onOpenFile}
          />
        </TabsContent>

        <TabsContent value="backups" className="flex flex-col p-5">
          <ServerBackupsTab id={id} running={running} config={config.data} />
        </TabsContent>

        <TabsContent value="settings" className="p-5">
          <ServerSettingsTab
            server={server}
            config={config.data}
            running={running}
          />
        </TabsContent>
      </Tabs>

      <ContentInstallDialog
        entry={serverTarget(server)}
        files={install.files}
        open={install.open}
        onOpenChange={install.onOpenChange}
      />
    </div>
  );
}
