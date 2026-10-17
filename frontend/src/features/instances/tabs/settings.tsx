import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, InstanceInfo } from '@/api';
import { InstanceSyncField } from '@/features/instances/components';
import {
  DangerSettings,
  GeneralSettings,
  JavaSettings,
  SettingsTabs,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { instanceMutations } from '@/queries/instance';
import { useJobMutation } from '@/queries/jobs';

export function InstanceSettingsTab({
  instance,
  config,
  running,
}: {
  instance: InstanceInfo;
  config?: ConfigEntry[];
  running: boolean;
}) {
  const globalConfig = useQuery(configQueries.list());
  const rename = useMutation(instanceMutations.rename(instance.id));
  const setConfig = useMutation(instanceMutations.setConfig(instance.id));
  const remove = useMutation(instanceMutations.remove(instance.id));
  const update = useJobMutation(instanceMutations.update(instance.id));

  return (
    <SettingsTabs
      tabs={[
        {
          id: 'general',
          label: m['entry.settings.section.general'](),
          content: (
            <>
              <GeneralSettings
                kind="instance"
                entry={instance}
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
                kind="instance"
                name={instance.name}
                running={running}
                remove={remove}
              />
            </>
          ),
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
          id: 'sync',
          label: m['instance.sync.title'](),
          content: <InstanceSyncField id={instance.id} />,
        },
      ]}
    />
  );
}
