import {
  ArrowsLeftRightIcon,
  CoffeeIcon,
  GearSixIcon,
  WarningIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, InstanceInfo } from '@/api';
import { InstanceSyncField } from '@/features/instances/components';
import {
  DangerSection,
  GeneralSection,
  JavaSection,
  SettingsLayout,
  SettingsSection,
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
    <SettingsLayout
      sections={[
        {
          id: 'general',
          label: m['entry.settings.section.general'](),
          icon: GearSixIcon,
          content: (
            <GeneralSection
              kind="instance"
              entry={instance}
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
              versionsQuery={instanceQueries.versions(instance.flavor)}
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
          id: 'sync',
          label: m['instance.sync.title'](),
          icon: ArrowsLeftRightIcon,
          content: (
            <SettingsSection title={m['instance.sync.title']()}>
              <InstanceSyncField id={instance.id} />
            </SettingsSection>
          ),
        },
        {
          id: 'danger',
          label: m['entry.settings.section.danger'](),
          icon: WarningIcon,
          destructive: true,
          content: (
            <DangerSection
              kind="instance"
              name={instance.name}
              running={running}
              remove={remove}
            />
          ),
        },
      ]}
    />
  );
}
