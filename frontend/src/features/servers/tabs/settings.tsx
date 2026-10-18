import type { ConfigEntry, ServerInfo } from '@/api';
import { PropertiesSettings, TaskSettings } from '@/features/servers/settings';
import {
  SettingsTabs,
  useEntrySettingsTabs,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
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
  const { general, java } = useEntrySettingsTabs({
    kind: 'server',
    entry: server,
    config,
    running,
    update: useJobMutation(serverMutations.update(server.id)),
  });

  return (
    <SettingsTabs
      tabs={[
        general,
        {
          id: 'properties',
          label: m['server.properties.title'](),
          content: <PropertiesSettings id={server.id} running={running} />,
        },
        java,
        {
          id: 'tasks',
          label: m['server.tasks.tab'](),
          content: <TaskSettings id={server.id} />,
        },
      ]}
    />
  );
}
