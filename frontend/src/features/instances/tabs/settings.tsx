import type { ConfigEntry, InstanceInfo } from '@/api';
import { InstanceSyncField } from '@/features/instances/components';
import {
  SettingsTabs,
  useEntrySettingsTabs,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
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
  const { general, java } = useEntrySettingsTabs({
    kind: 'instance',
    entry: instance,
    config,
    running,
    update: useJobMutation(instanceMutations.update(instance.id)),
  });

  return (
    <SettingsTabs
      tabs={[
        general,
        java,
        {
          id: 'sync',
          label: m['instance.sync.title'](),
          content: <InstanceSyncField id={instance.id} />,
        },
      ]}
    />
  );
}
