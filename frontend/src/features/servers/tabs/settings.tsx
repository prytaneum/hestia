import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, ServerInfo } from '@/api';
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useDraft } from '@/features/shared/entry/hooks';
import {
  configValue,
  EntrySettingsTab,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { useJobMutation } from '@/queries/jobs';
import { serverMutations, serverQueries } from '@/queries/server';

const INTERVALS = ['off', '6h', '12h', '1d'];

const intervalOptions = () =>
  INTERVALS.map((iv) => ({
    value: iv,
    label:
      iv === 'off'
        ? m['app.label.off']()
        : m['entry.settings.every_interval']({ interval: iv }),
  }));

/** Scheduled backups — the one setting an instance has no equivalent for. */
function useBackupSchedule(config?: ConfigEntry[]) {
  const loaded = config !== undefined;
  const storedInterval = configValue(config, 'backup-interval') || 'off';
  const storedRetention = configValue(config, 'backup-retention') || '10';

  const [interval, setInterval] = useDraft(
    storedInterval,
    `${loaded}:${storedInterval}`,
  );
  const [retention, setRetention] = useDraft(
    storedRetention,
    `${loaded}:${storedRetention}`,
  );

  const intervals = intervalOptions();

  const fields = (
    <div className="grid gap-4 sm:grid-cols-2">
      <Field>
        <FieldLabel htmlFor="backup-interval">
          {m['entry.settings.backup_schedule']()}
        </FieldLabel>
        <Select
          items={intervals}
          value={interval}
          onValueChange={(v) => setInterval(v ?? 'off')}
        >
          <SelectTrigger id="backup-interval" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {intervals.map((iv) => (
              <SelectItem key={iv.value} value={iv.value}>
                {iv.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Field>
        <FieldLabel htmlFor="backup-retention">
          {m['entry.settings.keep_backups']()}
        </FieldLabel>
        <Input
          id="backup-retention"
          type="number"
          min={1}
          value={retention}
          onChange={(e) => setRetention(e.target.value)}
        />
      </Field>
    </div>
  );

  const entries: ConfigEntry[] = [
    { key: 'backup-interval', value: interval === 'off' ? '' : interval },
    { key: 'backup-retention', value: retention },
  ];

  return { fields, entries };
}

/** Written only when edited, so Apply never rewrites a seed the user left alone. */
function useWorldSeed(config?: ConfigEntry[]) {
  const loaded = config !== undefined;
  const stored = configValue(config, 'level-seed');
  const [seed, setSeed] = useDraft(stored, `${loaded}:${stored}`);

  const fields = (
    <Field>
      <FieldLabel htmlFor="level-seed">
        {m['entry.create.properties.seed']()}
      </FieldLabel>
      <Input
        id="level-seed"
        value={seed}
        onChange={(e) => setSeed(e.target.value)}
        placeholder={m['entry.create.properties.seed_random']()}
        className="font-mono"
      />
      <FieldDescription>{m['entry.settings.seed_hint']()}</FieldDescription>
    </Field>
  );

  const trimmed = seed.trim();
  const entries: ConfigEntry[] =
    trimmed === stored ? [] : [{ key: 'level-seed', value: trimmed }];

  return { fields, entries };
}

export function ServerSettingsTab({
  server,
  config,
  running,
}: {
  server: ServerInfo;
  config?: ConfigEntry[];
  running: boolean;
}) {
  const globalConfig = useQuery(configQueries.list());
  const rename = useMutation(serverMutations.rename(server.id));
  const setConfig = useMutation(serverMutations.setConfig(server.id));
  const remove = useMutation(serverMutations.remove(server.id));
  const update = useJobMutation(serverMutations.update(server.id));
  const backups = useBackupSchedule(config);
  const seed = useWorldSeed(config);

  return (
    <EntrySettingsTab
      kind="server"
      entry={server}
      config={config}
      defaults={launcherDefaults(globalConfig.data)}
      running={running}
      rename={{
        mutate: (name) =>
          rename.mutate(name, {
            onSuccess: (updated) =>
              toast.success(m['app.toast.renamed']({ name: updated.name })),
          }),
        isPending: rename.isPending,
      }}
      setConfig={setConfig}
      remove={remove}
      update={update}
      versionsQuery={serverQueries.versions(server.flavor)}
      extraFields={
        <>
          {seed.fields}
          {backups.fields}
        </>
      }
      extraConfig={[...seed.entries, ...backups.entries]}
    />
  );
}
