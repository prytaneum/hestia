import { PluginSettingsSections } from '@/features/plugins';
import {
  PluginDeveloper,
  PluginsPanel,
  RepositoryList,
  Setting,
  SettingsSection,
  useFiltering,
} from '@/features/settings/components';
import { useConfig } from '@/features/settings/use-config';
import { m } from '@/paraglide/messages.js';

/** `open` is the plugin whose row starts expanded, for a link to its settings. */
export function PluginsTab({ open }: { open?: string }) {
  const filtering = useFiltering();
  const { entries } = useConfig();
  return (
    <>
      <SettingsSection group="plugins" legend={m['settings.tabs.plugins']()}>
        <Setting id="plugins">
          <PluginsPanel open={open} />
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
      {entries.developer && (
        <SettingsSection
          group="plugin-developer"
          legend={m['settings.plugins.developer.title']()}
        >
          <Setting id="plugin-developer">
            <PluginDeveloper />
          </Setting>
        </SettingsSection>
      )}
      {!filtering && <PluginSettingsSections />}
    </>
  );
}
