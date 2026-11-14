import {
  ModpackSettings,
  Setting,
  SettingsSection,
} from '@/features/settings/components';
import { useConfig } from '@/features/settings/use-config';
import { m } from '@/paraglide/messages.js';

export function ContentTab() {
  const { entries, pending, commit } = useConfig();

  return (
    <SettingsSection
      group="modpack"
      legend={m['settings.modpack.title']()}
      description={m['settings.modpack.hint']()}
    >
      <Setting id="modpack">
        <ModpackSettings
          config={entries.modpack ?? {}}
          pending={pending}
          onCommit={commit}
        />
      </Setting>
    </SettingsSection>
  );
}
