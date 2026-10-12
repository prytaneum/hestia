import type { ConfigEntry } from '@/api';
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Slider } from '@/components/ui/slider';
import { memGb } from '@/lib/format';
import { m } from '@/paraglide/messages.js';
import type { LauncherDefaults } from '@/queries/config';
import { useDraft } from '../hooks';
import { configValue, type SetConfig, saveConfig } from './config';
import { SaveBar, SettingsSection } from './layout';

/** Memory and JVM arguments, each inheriting the launcher default until set. */
export function JavaSection({
  config,
  defaults,
  running,
  setConfig,
}: {
  config?: ConfigEntry[];
  defaults: LauncherDefaults;
  running: boolean;
  setConfig: SetConfig;
}) {
  const defaultMemory = defaults.memory ?? '';
  const defaultMemoryGb = defaultMemory ? memGb(defaultMemory) : 4;
  const memoryOverride = configValue(config, 'memory');
  const storedMemory = memoryOverride ? memGb(memoryOverride) : defaultMemoryGb;
  const storedJvmArgs = configValue(config, 'jvm-args');
  const loaded = config !== undefined;

  const [memory, setMemory] = useDraft(
    storedMemory,
    `${loaded}:${memoryOverride}:${defaultMemoryGb}`,
  );
  const [jvmArgs, setJvmArgs] = useDraft(
    storedJvmArgs,
    `${loaded}:${storedJvmArgs}`,
  );

  const inherits = defaultMemory !== '' && memory === defaultMemoryGb;
  const dirty = memory !== storedMemory || jvmArgs !== storedJvmArgs;

  const save = () =>
    saveConfig(setConfig, [
      // Matching the launcher default clears the override so it keeps inheriting.
      { key: 'memory', value: inherits ? '' : `${memory}G` },
      { key: 'jvm-args', value: jvmArgs },
    ]);

  return (
    <>
      <SettingsSection
        title={m['app.label.java']()}
        description={m['entry.settings.section.java_description']()}
      >
        <FieldGroup>
          <Field>
            <FieldLabel>
              {m['entry.settings.allocated_memory']()}
              <span className="ml-2 font-mono text-muted-foreground">
                {m['entry.create.gb']({ value: memory })}
                {inherits && ` (${m['entry.settings.inherits_default']()})`}
              </span>
            </FieldLabel>
            <Slider
              value={[memory]}
              min={2}
              max={32}
              step={1}
              onValueChange={(v) => setMemory(Array.isArray(v) ? v[0] : v)}
              className="max-w-md"
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="jvm-args">
              {m['entry.settings.java_arguments']()}
            </FieldLabel>
            <Input
              id="jvm-args"
              value={jvmArgs}
              onChange={(e) => setJvmArgs(e.target.value)}
              placeholder={defaults['jvm-args'] || '-XX:+UseG1GC'}
              className="font-mono"
            />
          </Field>
        </FieldGroup>
      </SettingsSection>
      <SaveBar
        dirty={dirty}
        pending={setConfig.isPending}
        running={running}
        onSave={save}
        onDiscard={() => {
          setMemory(storedMemory);
          setJvmArgs(storedJvmArgs);
        }}
      />
    </>
  );
}
