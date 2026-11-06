import { PluginSettingsSections } from '@/features/plugins';
import {
  PluginsPanel,
  RepositoryList,
  Setting,
  SettingsSection,
  useFiltering,
} from '@/features/settings/components';
import { m } from '@/paraglide/messages.js';

export function PluginsTab() {
  const filtering = useFiltering();
  return (
    <>
      <SettingsSection group="plugins" legend={m['settings.tabs.plugins']()}>
        <Setting id="plugins">
          <PluginsPanel />
        </Setting>
      </SettingsSection>
      <SettingsSection
        group="plugin-repositories"
        legend={m['settings.plugins.repos.title']()}
      >
        <Setting id="plugin-repositories">
          <RepositoryList />
        </Setting>
      </SettingsSection>
      {!filtering && <PluginSettingsSections />}
    </>
  );
}
