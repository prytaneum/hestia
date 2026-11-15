import {
  ModpackSettings,
  Setting,
  SettingsSection,
  SwitchRow,
} from '@/features/settings/components';
import { useConfig } from '@/features/settings/use-config';
import { m } from '@/paraglide/messages.js';

export function ContentTab() {
  const { entries, pending, commit } = useConfig();

  return (
    <>
      <SettingsSection
        group="content-updates"
        legend={m['settings.content_updates.section']()}
      >
        <Setting id="auto-update">
          <SwitchRow
            id="auto-update"
            label={m['settings.content_updates.auto_update_label']()}
            description={m[
              'settings.content_updates.auto_update_description'
            ]()}
            checked={entries.content?.['auto-update'] ?? false}
            onChange={(checked) => commit('content.auto-update', checked)}
          />
        </Setting>
      </SettingsSection>

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
    </>
  );
}
