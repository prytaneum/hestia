import { ArrowsClockwiseIcon, TrashIcon } from '@phosphor-icons/react';
import type { UseQueryOptions } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { type ReactNode, useState } from 'react';
import { toast } from 'sonner';

import type { ConfigEntry, GameVersion } from '@/api';
import { errorMessage } from '@/api';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  Field,
  FieldGroup,
  FieldLabel,
  FieldSeparator,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Slider } from '@/components/ui/slider';
import { memGb } from '@/lib/format';
import { m } from '@/paraglide/messages.js';
import type { JobEntryKind } from '@/queries';
import type { LauncherDefaults } from '@/queries/config';
import { ChangeVersionDialog, type UpdateHandle } from './dialogs';
import { useDraft } from './hooks';

export function configValue(
  config: ConfigEntry[] | undefined,
  key: string,
): string {
  return config?.find((e) => e.key === key)?.value ?? '';
}

export interface EntrySettingsProps {
  kind: JobEntryKind;
  entry: { id: string; name: string; flavor: string; gameVersion: string };
  config?: ConfigEntry[];
  defaults: LauncherDefaults;
  running: boolean;
  rename: { mutate: (name: string) => void; isPending: boolean };
  setConfig: {
    mutateAsync: (entry: ConfigEntry) => Promise<unknown>;
    isPending: boolean;
  };
  remove: { mutate: (v: undefined, o: { onSuccess: () => void }) => void };
  update: UpdateHandle;
  // biome-ignore lint/suspicious/noExplicitAny: the query factories' option types differ per domain.
  versionsQuery: UseQueryOptions<GameVersion[], any, GameVersion[], any>;
  /** Rendered under the shared fields; a server's backup schedule lives here. */
  extraFields?: ReactNode;
  /** Written alongside the shared values when Apply is pressed. */
  extraConfig?: ConfigEntry[];
  /** Blocks Apply while an extra field holds a value that cannot be saved. */
  extraInvalid?: boolean;
}

/**
 * The settings a server and an instance share: rename, memory, JVM args, an
 * in-place version change and removal. What only one of them has arrives
 * through `extraFields`/`extraConfig`.
 */
export function EntrySettingsTab({
  kind,
  entry,
  config,
  defaults,
  running,
  rename,
  setConfig,
  remove,
  update,
  versionsQuery,
  extraFields,
  extraConfig,
  extraInvalid = false,
}: EntrySettingsProps) {
  const navigate = useNavigate();
  const server = kind === 'server';

  const defaultMemory = defaults.memory ?? '';
  const defaultMemoryGb = defaultMemory ? memGb(defaultMemory) : 4;
  const defaultJvmArgs = defaults['jvm-args'] ?? '';
  const memoryOverride = configValue(config, 'memory');
  const inheritsMemory = memoryOverride === '' && defaultMemory !== '';

  const [name, setName] = useDraft(entry.name, entry.name);
  const loaded = config !== undefined;
  const [memory, setMemory] = useDraft(
    memoryOverride ? memGb(memoryOverride) : defaultMemoryGb,
    `${loaded}:${memoryOverride}:${defaultMemoryGb}`,
  );
  const [jvmArgs, setJvmArgs] = useDraft(
    configValue(config, 'jvm-args'),
    `${loaded}:${configValue(config, 'jvm-args')}`,
  );
  const [changing, setChanging] = useState(false);

  const saveConfig = async () => {
    // Match the launcher default → clear the override so it keeps inheriting.
    const memoryValue =
      defaultMemory !== '' && memory === defaultMemoryGb ? '' : `${memory}G`;
    try {
      await setConfig.mutateAsync({ key: 'memory', value: memoryValue });
      await setConfig.mutateAsync({ key: 'jvm-args', value: jvmArgs });
      for (const entryConfig of extraConfig ?? []) {
        await setConfig.mutateAsync(entryConfig);
      }
      toast.success(m['app.toast.saved']());
    } catch (error) {
      toast.error(errorMessage(error));
    }
  };

  const doRename = () => {
    const trimmed = name.trim();
    if (!trimmed || trimmed === entry.name) return;
    rename.mutate(trimmed);
  };

  return (
    <div className="max-w-lg">
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="entry-name">
            {server
              ? m['entry.settings.server_name']()
              : m['entry.settings.instance_name']()}
          </FieldLabel>
          <div className="flex gap-2">
            <Input
              id="entry-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={running}
            />
            <Button
              variant="outline"
              onClick={doRename}
              disabled={running || rename.isPending || name === entry.name}
            >
              {m['app.action.apply']()}
            </Button>
          </div>
        </Field>

        <Field>
          <FieldLabel>
            {m['entry.settings.allocated_memory']()}
            <span className="ml-2 font-mono text-muted-foreground">
              {m['entry.create.gb']({ value: memory })}
              {inheritsMemory && ` (${m['entry.settings.inherits_default']()})`}
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
            placeholder={defaultJvmArgs || '-XX:+UseG1GC'}
            className="font-mono"
          />
        </Field>

        {extraFields}

        <div>
          <Button
            onClick={saveConfig}
            disabled={setConfig.isPending || extraInvalid}
          >
            {m['app.action.apply']()}
          </Button>
        </div>

        <FieldSeparator />

        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            data-icon="inline-start"
            disabled={running}
            onClick={() => setChanging(true)}
          >
            <ArrowsClockwiseIcon />
            {m['entry.settings.change_version']()}
          </Button>
          <ConfirmDialog
            trigger={
              <Button
                variant="destructive"
                size="sm"
                data-icon="inline-start"
                disabled={running}
              >
                <TrashIcon />
                {server
                  ? m['entry.settings.remove.server']()
                  : m['entry.settings.remove.instance']()}
              </Button>
            }
            title={
              server
                ? m['entry.settings.remove.server_title']()
                : m['entry.settings.remove.instance_title']()
            }
            description={m['entry.settings.remove.description']({
              name: entry.name,
            })}
            destructive
            confirmLabel={
              server
                ? m['entry.settings.remove.server']()
                : m['entry.settings.remove.instance']()
            }
            onConfirm={() =>
              remove.mutate(undefined, {
                onSuccess: () => {
                  toast.success(m['app.toast.removed']({ name: entry.name }));
                  navigate({ to: server ? '/servers' : '/instances' });
                },
              })
            }
          />
        </div>
      </FieldGroup>

      <ChangeVersionDialog
        name={entry.name}
        gameVersion={entry.gameVersion}
        versionsQuery={versionsQuery}
        update={update}
        open={changing}
        onOpenChange={setChanging}
      />
    </div>
  );
}
