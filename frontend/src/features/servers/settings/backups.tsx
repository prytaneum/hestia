import type { ConfigEntry } from '@/api';
import { Field, FieldError, FieldLabel } from '@/components/ui/field';
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
  SaveBar,
  type SetConfig,
  SettingsSection,
  saveConfig,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';

const DEFAULT_RETENTION = '10';

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

/** Scheduled backups: how often one is taken and how many are kept. */
export function BackupScheduleSection({
  config,
  setConfig,
}: {
  config?: ConfigEntry[];
  setConfig: SetConfig;
}) {
  const loaded = config !== undefined;
  const storedInterval = configValue(config, 'backup-interval');
  const storedRetention =
    configValue(config, 'backup-retention') || DEFAULT_RETENTION;

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
  const dirty =
    intervalValue(interval) !== storedInterval || retention !== storedRetention;

  // Leaving a preset for custom keeps that preset's amount as the start.
  const setMode = (mode: string) =>
    setInterval(
      INTERVAL_PRESETS.includes(mode)
        ? intervalDraft(mode)
        : { ...interval, mode },
    );

  return (
    <>
      <SettingsSection
        title={m['app.label.backups']()}
        description={m['entry.settings.section.backups_description']()}
      >
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
      </SettingsSection>
      <SaveBar
        dirty={dirty}
        pending={setConfig.isPending}
        invalid={issue !== null}
        onSave={() =>
          saveConfig(setConfig, [
            { key: 'backup-interval', value: intervalValue(interval) },
            { key: 'backup-retention', value: retention },
          ])
        }
        onDiscard={() => {
          setInterval(intervalDraft(storedInterval));
          setRetention(storedRetention);
        }}
      />
    </>
  );
}
