import type { ConfigEntry } from '@/api';
import {
  SavedInput,
  SettingGroup,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Button } from '@/components/ui/button';
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
  entryConfig,
  type SetConfig,
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
export function BackupSettings({
  config,
  setConfig,
}: {
  config?: ConfigEntry[];
  setConfig: SetConfig;
}) {
  const { commit, save } = entryConfig(setConfig);
  const loaded = config !== undefined;
  const stored = configValue(config, 'backup-interval');
  const [interval, setInterval] = useDraft(
    intervalDraft(stored),
    `${loaded}:${stored}`,
  );

  const intervals = intervalOptions();
  const units = unitOptions();
  const issue = intervalIssue(interval);
  const custom = interval.mode === 'custom';
  const dirty = custom && intervalValue(interval) !== stored;

  // A preset or Off lands the moment it is picked; a custom amount is typed,
  // so it is saved on purpose. Leaving a preset for custom keeps its amount.
  const setMode = (mode: string) => {
    if (mode === 'custom') {
      setInterval({ ...interval, mode });
      return;
    }
    const next = intervalDraft(mode === 'off' ? '' : mode);
    setInterval(next);
    commit('backup-interval', intervalValue(next));
  };

  return (
    <SettingGroup legend={m['app.label.backups']()}>
      <SettingList>
        <SettingRow
          label={m['entry.settings.backup_schedule']()}
          description={
            issue ? (
              <span className="text-destructive">{issueMessage(issue)}</span>
            ) : (
              m['entry.settings.backup_schedule_hint']()
            )
          }
        >
          {dirty && issue === null && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => save('backup-interval', intervalValue(interval))}
            >
              {m['app.action.save']()}
            </Button>
          )}
          {custom && (
            <>
              <Input
                type="number"
                min={1}
                aria-label={m['entry.settings.interval.every']()}
                value={interval.count}
                onChange={(e) =>
                  setInterval({ ...interval, count: e.target.value })
                }
                aria-invalid={issue !== null}
                className="w-16"
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
            </>
          )}
          <Select
            items={intervals}
            value={interval.mode}
            onValueChange={(v) => setMode(v ?? 'off')}
          >
            <SelectTrigger
              aria-label={m['entry.settings.backup_schedule']()}
              className="w-36"
            >
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
        </SettingRow>

        <SettingRow
          htmlFor="backup-retention"
          label={m['entry.settings.keep_backups']()}
          description={m['entry.settings.keep_backups_hint']()}
        >
          <SavedInput
            id="backup-retention"
            trailing
            type="number"
            value={configValue(config, 'backup-retention') || DEFAULT_RETENTION}
            onSave={(value) => save('backup-retention', value)}
          />
        </SettingRow>
      </SettingList>
    </SettingGroup>
  );
}
