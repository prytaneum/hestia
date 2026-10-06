import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { ConfigEntry, ServerInfo } from '@/api';
import {
  Field,
  FieldDescription,
  FieldError,
  FieldLabel,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  INTERVAL_PRESETS,
  type IntervalIssue,
  type IntervalUnit,
  intervalDraft,
  intervalIssue,
  intervalValue,
} from '@/features/servers/lib';
import { useDraft } from '@/features/shared/entry/hooks';
import {
  configValue,
  EntrySettingsTab,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { useJobMutation } from '@/queries/jobs';
import { serverMutations, serverQueries } from '@/queries/server';

const intervalOptions = () => [
  { value: 'off', label: m['app.label.off']() },
  ...INTERVAL_PRESETS.map((iv) => ({
    value: iv,
    label: m['entry.settings.every_interval']({ interval: iv }),
  })),
  { value: 'custom', label: m['entry.settings.interval.custom']() },
];

const unitOptions = (): { value: IntervalUnit; label: string }[] => [
  { value: 'm', label: m['entry.settings.interval.minutes']() },
  { value: 'h', label: m['entry.settings.interval.hours']() },
  { value: 'd', label: m['entry.settings.interval.days']() },
];

const issueMessage = (issue: IntervalIssue) =>
  issue === 'too_short'
    ? m['entry.settings.interval.too_short']()
    : m['app.validation.whole_number']();

/** Scheduled backups — the one setting an instance has no equivalent for. */
function useBackupSchedule(config?: ConfigEntry[]) {
  const loaded = config !== undefined;
  const storedInterval = configValue(config, 'backup-interval');
  const storedRetention = configValue(config, 'backup-retention') || '10';

  const [interval, setInterval] = useDraft(
    intervalDraft(storedInterval),
    `${loaded}:${storedInterval}`,
  );
  const [retention, setRetention] = useDraft(
    storedRetention,
    `${loaded}:${storedRetention}`,
  );

  const intervals = intervalOptions();
  const units = unitOptions();
  const issue = intervalIssue(interval);

  // Leaving a preset for custom keeps that preset's amount as the start.
  const setMode = (mode: string) =>
    setInterval(
      INTERVAL_PRESETS.includes(mode)
        ? intervalDraft(mode)
        : { ...interval, mode },
    );

  const fields = (
    <div className="flex flex-col gap-3">
      <div className="grid gap-4 sm:grid-cols-2">
        <Field>
          <FieldLabel htmlFor="backup-interval">
            {m['entry.settings.backup_schedule']()}
          </FieldLabel>
          <Select
            items={intervals}
            value={interval.mode}
            onValueChange={(v) => setMode(v ?? 'off')}
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
      {interval.mode === 'custom' && (
        <Field data-invalid={issue !== null}>
          <div className="flex items-center gap-2">
            <FieldLabel htmlFor="backup-interval-count" className="w-auto">
              {m['entry.settings.interval.every']()}
            </FieldLabel>
            <Input
              id="backup-interval-count"
              type="number"
              min={1}
              value={interval.count}
              onChange={(e) =>
                setInterval({ ...interval, count: e.target.value })
              }
              aria-invalid={issue !== null}
              className="w-20"
            />
            <Select
              items={units}
              value={interval.unit}
              onValueChange={(v) =>
                setInterval({ ...interval, unit: v ?? interval.unit })
              }
            >
              <SelectTrigger
                aria-label={m['entry.settings.interval.unit']()}
                className="w-28"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {units.map((unit) => (
                  <SelectItem key={unit.value} value={unit.value}>
                    {unit.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {issue && <FieldError>{issueMessage(issue)}</FieldError>}
        </Field>
      )}
    </div>
  );

  const entries: ConfigEntry[] = [
    { key: 'backup-interval', value: intervalValue(interval) },
    { key: 'backup-retention', value: retention },
  ];

  return { fields, entries, invalid: issue !== null };
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
      extraInvalid={backups.invalid}
    />
  );
}
