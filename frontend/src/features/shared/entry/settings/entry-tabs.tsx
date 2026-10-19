import { useMutation, useQuery } from '@tanstack/react-query';
import type { ConfigEntry, EntryKind } from '@/api';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { entryMutations } from '@/queries/entry';
import type { UpdateHandle } from '../dialogs';
import { DangerSettings } from './danger';
import { GeneralSettings } from './general';
import { JavaSettings } from './java';
import type { SettingsTabDef } from './tabs';

/** The general and java tabs both kinds share; each feature composes its own order. */
export function useEntrySettingsTabs({
  kind,
  entry,
  config,
  running,
  update,
}: {
  kind: EntryKind;
  entry: { id: string; name: string; flavor: string; gameVersion: string };
  config?: ConfigEntry[];
  running: boolean;
  update: UpdateHandle;
}): { general: SettingsTabDef; java: SettingsTabDef } {
  const globalConfig = useQuery(configQueries.list());
  const rename = useMutation(entryMutations.rename(kind, entry.id));
  const setConfig = useMutation(entryMutations.setConfig(kind, entry.id));
  const remove = useMutation(entryMutations.remove(kind, entry.id));

  return {
    general: {
      id: 'general',
      label: m['entry.settings.section.general'](),
      content: (
        <>
          <GeneralSettings
            kind={kind}
            entry={entry}
            running={running}
            rename={rename.mutateAsync}
            update={update}
          />
          <DangerSettings
            kind={kind}
            name={entry.name}
            running={running}
            remove={remove}
          />
        </>
      ),
    },
    java: {
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
  };
}
