import {
  PluginList,
  Setting,
  SettingsSection,
} from '@/features/settings/components';
import { m } from '@/paraglide/messages.js';

export function PluginsTab() {
  return (
    <SettingsSection group="plugins" legend={m['settings.tabs.plugins']()}>
      <Setting id="plugins">
        <PluginList />
      </Setting>
    </SettingsSection>
  );
}
