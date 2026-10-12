import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, ServerInfo } from '@/api';
import {
  BackupSettings,
  PropertiesSettings,
} from '@/features/servers/settings';
import {
  DangerSettings,
  GeneralSettings,
  JavaSettings,
  SettingsTabs,
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
    <SettingsTabs
      tabs={[
        {
          id: 'general',
          label: m['entry.settings.section.general'](),
          content: (
            <>
              <GeneralSettings
                kind="server"
                entry={server}
                running={running}
                rename={(name) =>
                  rename.mutate(name, {
                    onSuccess: (updated) =>
                      toast.success(
                        m['app.toast.renamed']({ name: updated.name }),
                      ),
                  })
                }
                update={update}
                versionsQuery={serverQueries.versions(server.flavor)}
              />
              <DangerSettings
                kind="server"
                name={server.name}
                running={running}
                remove={remove}
              />
            </>
          ),
        },
        {
          id: 'properties',
          label: m['server.properties.title'](),
          content: <PropertiesSettings id={server.id} running={running} />,
        },
        {
          id: 'java',
          label: m['app.label.java'](),
          content: (
            <JavaSettings
              config={config}
              defaults={launcherDefaults(globalConfig.data)}
              setConfig={setConfig}
            />
          ),
        },
        {
          id: 'backups',
          label: m['app.label.backups'](),
          content: <BackupSettings config={config} setConfig={setConfig} />,
        },
      ]}
    />
  );
}
