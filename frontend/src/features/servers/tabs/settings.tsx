import {
  ArchiveIcon,
  CoffeeIcon,
  GearSixIcon,
  SlidersHorizontalIcon,
  WarningIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, ServerInfo } from '@/api';
import {
  BackupScheduleSection,
  PropertiesSection,
} from '@/features/servers/settings';
import {
  DangerSection,
  GeneralSection,
  JavaSection,
  SettingsLayout,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { useJobMutation } from '@/queries/jobs';
import { serverMutations, serverQueries } from '@/queries/server';

export function ServerSettingsTab({
  server,
  config,
  running,
}: {
  server: ServerInfo;
  config?: ConfigEntry[];
  running: boolean;
}) {
  const globalConfig = useQuery(configQueries.list());
  const rename = useMutation(serverMutations.rename(server.id));
  const setConfig = useMutation(serverMutations.setConfig(server.id));
  const remove = useMutation(serverMutations.remove(server.id));
  const update = useJobMutation(serverMutations.update(server.id));

  return (
    <SettingsLayout
      sections={[
        {
          id: 'general',
          label: m['entry.settings.section.general'](),
          icon: GearSixIcon,
          content: (
            <GeneralSection
              kind="server"
              entry={server}
              running={running}
              rename={{
                mutate: (name) =>
                  rename.mutate(name, {
                    onSuccess: (updated) =>
                      toast.success(
                        m['app.toast.renamed']({ name: updated.name }),
                      ),
                  }),
                isPending: rename.isPending,
              }}
              update={update}
              versionsQuery={serverQueries.versions(server.flavor)}
            />
          ),
        },
        {
          id: 'properties',
          label: m['server.properties.title'](),
          icon: SlidersHorizontalIcon,
          content: (
            <PropertiesSection
              config={config}
              running={running}
              setConfig={setConfig}
            />
          ),
        },
        {
          id: 'java',
          label: m['app.label.java'](),
          icon: CoffeeIcon,
          content: (
            <JavaSection
              config={config}
              defaults={launcherDefaults(globalConfig.data)}
              running={running}
              setConfig={setConfig}
            />
          ),
        },
        {
          id: 'backups',
          label: m['app.label.backups'](),
          icon: ArchiveIcon,
          content: (
            <BackupScheduleSection config={config} setConfig={setConfig} />
          ),
        },
        {
          id: 'danger',
          label: m['entry.settings.section.danger'](),
          icon: WarningIcon,
          destructive: true,
          content: (
            <DangerSection
              kind="server"
              name={server.name}
              running={running}
              remove={remove}
            />
          ),
        },
      ]}
    />
  );
}
