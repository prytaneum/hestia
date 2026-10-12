import { useState } from 'react';

import type { ConfigEntry } from '@/api';
import {
  SavedInput,
  SettingGroup,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Slider } from '@/components/ui/slider';
import { memGb } from '@/lib/format';
import { m } from '@/paraglide/messages.js';
import type { LauncherDefaults } from '@/queries/config';
import { configValue, entryConfig, type SetConfig } from './config';

/** Memory and JVM arguments, each following the launcher default until set. */
export function JavaSettings({
  config,
  defaults,
  setConfig,
}: {
  config?: ConfigEntry[];
  defaults: LauncherDefaults;
  setConfig: SetConfig;
}) {
  const { commit, save } = entryConfig(setConfig);
  const defaultMemory = defaults.memory ?? '';
  const defaultMemoryGb = defaultMemory ? memGb(defaultMemory) : 4;
  const override = configValue(config, 'memory');

  const [draft, setDraft] = useState<number | null>(null);
  const memory = draft ?? (override ? memGb(override) : defaultMemoryGb);
  const inherits = defaultMemory !== '' && memory === defaultMemoryGb;

  return (
    <SettingGroup
      legend={m['app.label.java']()}
      description={m['entry.settings.applies_on_start']()}
    >
      <SettingList>
        <SettingRow
          label={
            <>
              {m['entry.settings.allocated_memory']()}
              <span className="font-mono text-muted-foreground">
                {m['entry.create.gb']({ value: memory })}
              </span>
            </>
          }
          description={
            inherits ? m['entry.settings.follows_default']() : undefined
          }
        >
          <Slider
            aria-label={m['entry.settings.allocated_memory']()}
            className="data-horizontal:w-64"
            min={2}
            max={32}
            step={1}
            value={memory}
            onValueChange={(v) => setDraft(Array.isArray(v) ? v[0] : v)}
            onValueCommitted={(v) => {
              const next = Array.isArray(v) ? v[0] : v;
              setDraft(null);
              // Matching the launcher default clears the override so it keeps
              // inheriting.
              commit(
                'memory',
                defaultMemory !== '' && next === defaultMemoryGb
                  ? ''
                  : `${next}G`,
              );
            }}
          />
        </SettingRow>

        <SettingRow
          htmlFor="entry-jvm-args"
          label={m['entry.settings.java_arguments']()}
        >
          <SavedInput
            id="entry-jvm-args"
            trailing
            mono
            value={configValue(config, 'jvm-args')}
            placeholder={defaults['jvm-args'] || '-XX:+UseG1GC'}
            onSave={(value) => save('jvm-args', value)}
          />
        </SettingRow>
      </SettingList>
    </SettingGroup>
  );
}
