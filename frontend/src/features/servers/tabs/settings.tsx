import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, ServerInfo } from '@/api';
import { PropertiesSettings, TaskSettings } from '@/features/servers/settings';
import {
  DangerSettings,
  GeneralSettings,
  JavaSettings,
  SettingsTabs,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { entryMutations } from '@/queries/entry';
import { useJobMutation } from '@/queries/jobs';
import { serverMutations } from '@/queries/server';

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
  const rename = useMutation(entryMutations.rename('server', server.id));
  const setConfig = useMutation(entryMutations.setConfig('server', server.id));
  const remove = useMutation(entryMutations.remove('server', server.id));
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
          id: 'tasks',
          label: m['server.tasks.tab'](),
          content: <TaskSettings id={server.id} />,
        },
      ]}
    />
  );
}
