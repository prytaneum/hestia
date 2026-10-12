import { useState } from 'react';

import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
import { Slider } from '@/components/ui/slider';
import {
  RuntimeList,
  SavedInput,
  Setting,
  SettingsSection,
} from '@/features/settings/components';
import { useConfig } from '@/features/settings/use-config';
import { memGb } from '@/lib/format';
import { m } from '@/paraglide/messages.js';
import { useMemoryRange } from '@/queries/host';

export function JavaTab() {
  const { entries, commit, save } = useConfig();

  const defaultMemory = entries.defaults?.memory ?? '';
  const [memoryDraft, setMemoryDraft] = useState<number | null>(null);
  const memoryRange = useMemoryRange();
  const memoryValue =
    memoryDraft ??
    Math.min(defaultMemory ? memGb(defaultMemory) : 4, memoryRange.max);

  return (
    <>
      <SettingsSection
        group="java-defaults"
        legend={m['settings.java.defaults']()}
      >
        <Setting id="default-memory">
          <Field>
            <FieldLabel htmlFor="default-memory">
              {m['settings.default_memory']()} —{' '}
              {defaultMemory || memoryDraft !== null
                ? m['entry.create.gb']({ value: memoryValue })
                : m['settings.no_default']()}
            </FieldLabel>
            <Slider
              id="default-memory"
              className="max-w-md"
              min={memoryRange.min}
              max={memoryRange.max}
              step={1}
              value={memoryValue}
              onValueChange={(v) => setMemoryDraft(Array.isArray(v) ? v[0] : v)}
              onValueCommitted={(v) => {
                setMemoryDraft(null);
                commit('defaults.memory', `${Array.isArray(v) ? v[0] : v}G`);
              }}
            />
            <FieldDescription>
              {m['settings.default_memory_hint']()}
            </FieldDescription>
          </Field>
        </Setting>

        <Setting id="jvm-args">
          <Field>
            <FieldLabel htmlFor="default-jvm-args">
              {m['settings.default_jvm_args']()}
            </FieldLabel>
            <SavedInput
              id="default-jvm-args"
              mono
              value={entries.defaults?.['jvm-args'] ?? ''}
              onSave={(value) => save('defaults.jvm-args', value)}
            />
          </Field>
        </Setting>
      </SettingsSection>

      <SettingsSection
        group="java-runtimes"
        legend={m['settings.java.runtimes']()}
      >
        <Setting id="runtimes">
          <RuntimeList />
        </Setting>
      </SettingsSection>
    </>
  );
}
