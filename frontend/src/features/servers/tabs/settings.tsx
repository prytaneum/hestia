import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, ServerInfo } from '@/api';
import { FieldGroup } from '@/components/ui/field';
import { BackupSettings } from '@/features/servers/settings';
import {
  DangerSettings,
  GeneralSettings,
  JavaSettings,
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
    <FieldGroup className="max-w-2xl gap-8">
      <GeneralSettings
        kind="server"
        entry={server}
        running={running}
        rename={(name) =>
          rename.mutate(name, {
            onSuccess: (updated) =>
              toast.success(m['app.toast.renamed']({ name: updated.name })),
          })
        }
        update={update}
        versionsQuery={serverQueries.versions(server.flavor)}
      />
      <JavaSettings
        config={config}
        defaults={launcherDefaults(globalConfig.data)}
        setConfig={setConfig}
      />
      <BackupSettings config={config} setConfig={setConfig} />
      <DangerSettings
        kind="server"
        name={server.name}
        running={running}
        remove={remove}
      />
    </FieldGroup>
  );
}
