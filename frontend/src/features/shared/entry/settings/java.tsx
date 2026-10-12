import { useState } from 'react';

import type { ConfigEntry } from '@/api';
import { SavedInput, SettingGroup } from '@/components/setting-controls';
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
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
    <SettingGroup legend={m['app.label.java']()}>
      <Field>
        <FieldLabel htmlFor="entry-memory">
          {m['entry.settings.allocated_memory']()} —{' '}
          {m['entry.create.gb']({ value: memory })}
          {inherits && ` (${m['entry.settings.inherits_default']()})`}
        </FieldLabel>
        <Slider
          id="entry-memory"
          className="max-w-md"
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
        <FieldDescription>
          {m['entry.settings.applies_on_start']()}
        </FieldDescription>
      </Field>

      <Field>
        <FieldLabel htmlFor="entry-jvm-args">
          {m['entry.settings.java_arguments']()}
        </FieldLabel>
        <SavedInput
          id="entry-jvm-args"
          mono
          value={configValue(config, 'jvm-args')}
          placeholder={defaults['jvm-args'] || '-XX:+UseG1GC'}
          onSave={(value) => save('jvm-args', value)}
        />
      </Field>
    </SettingGroup>
  );
}
