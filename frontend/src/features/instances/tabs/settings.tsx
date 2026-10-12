import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, InstanceInfo } from '@/api';
import { FieldGroup } from '@/components/ui/field';
import { InstanceSyncField } from '@/features/instances/components';
import {
  DangerSettings,
  GeneralSettings,
  JavaSettings,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { instanceMutations, instanceQueries } from '@/queries/instance';

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
  const update = useMutation(instanceMutations.update(instance.id));

  return (
    <FieldGroup className="max-w-2xl gap-8">
      <GeneralSettings
        kind="instance"
        entry={instance}
        running={running}
        rename={(name) =>
          rename.mutate(name, {
            onSuccess: (updated) =>
              toast.success(m['app.toast.renamed']({ name: updated.name })),
          })
        }
        update={update}
        versionsQuery={instanceQueries.versions(instance.flavor)}
      />
      <JavaSettings
        config={config}
        defaults={launcherDefaults(globalConfig.data)}
        setConfig={setConfig}
      />
      <InstanceSyncField id={instance.id} />
      <DangerSettings
        kind="instance"
        name={instance.name}
        running={running}
        remove={remove}
      />
    </FieldGroup>
  );
}
