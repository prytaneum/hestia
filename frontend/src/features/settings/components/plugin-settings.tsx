import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import type { PluginInfo, Setting, SettingValue } from '@/api';
import { ExternalLink } from '@/components/external-link';
import { SaveButton, SaveStatus } from '@/components/save-button';
import {
  SavedInput,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import {
  groupSettings,
  type SettingProblem,
  settingProblem,
} from '@/features/plugins/lib';
import { usePluginText } from '@/features/plugins/text';
import { useSaveState } from '@/hooks/use-save-state';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

/** The settings a plugin's manifest declares, one control per kind. */
export function PluginSettings({ plugin }: { plugin: PluginInfo }) {
  const { id, settings } = plugin.manifest;
  const values = useQuery(pluginQueries.settings(id));
  const save = useMutation(pluginMutations.setSetting(id));
  const text = usePluginText([id]);
  if (settings.length === 0) return null;

  const write = (key: string, value: unknown) =>
    save.mutateAsync({ key, value });

  return (
    <div className="space-y-1.5">
      <div className="font-medium text-xs">
        {m['settings.plugins.settings.title']()}
      </div>
      {groupSettings(settings).map(([group, rows]) => (
        <div key={group} className="space-y-1.5">
          {group && (
            <div className="text-[11px] text-muted-foreground">
              {text(id, `group.${group}`, group)}
            </div>
          )}
          <SettingList>
            {rows.map((setting) => (
              <SettingRow
                key={setting.key}
                htmlFor={`${id}-${setting.key}`}
                label={text(
                  id,
                  `setting.${setting.key}.label`,
                  setting.label || setting.key,
                )}
                description={
                  <>
                    {text(id, `setting.${setting.key}.help`, setting.help)}
                    {setting.helpUrl && (
                      <>
                        {setting.help && ' '}
                        <ExternalLink
                          href={setting.helpUrl}
                          className="underline underline-offset-2"
                        >
                          {m['settings.plugins.settings.help_link']()}
                        </ExternalLink>
                      </>
                    )}
                  </>
                }
              >
                <Control
                  id={`${id}-${setting.key}`}
                  setting={setting}
                  placeholder={text(
                    id,
                    `setting.${setting.key}.placeholder`,
                    setting.placeholder,
                  )}
                  current={values.data?.find((v) => v.key === setting.key)}
                  onSave={(value) => write(setting.key, value)}
                />
              </SettingRow>
            ))}
          </SettingList>
        </div>
      ))}
    </div>
  );
}

function problemText(problem: SettingProblem): string {
  switch (problem.kind) {
    case 'min':
      return m['settings.plugins.settings.problem.min']({ min: problem.min });
    case 'max':
      return m['settings.plugins.settings.problem.max']({ max: problem.max });
    case 'pattern':
      return m['settings.plugins.settings.problem.pattern']();
  }
}

function Control({
  id,
  setting,
  placeholder,
  current,
  onSave,
}: {
  id: string;
  setting: Setting;
  placeholder?: string;
  current?: SettingValue;
  onSave: (value: unknown) => Promise<unknown>;
}) {
  const value = current?.value;
  switch (setting.kind) {
    case 'bool':
      return (
        <Switch
          id={id}
          checked={value === true}
          onCheckedChange={(next) => void onSave(next === true)}
        />
      );
    case 'select':
      return (
        <Select
          value={typeof value === 'string' ? value : null}
          onValueChange={(next) => void onSave(next)}
        >
          <SelectTrigger id={id} className="w-44" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {setting.options.map((option) => (
              <SelectItem key={option} value={option}>
                {option}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      );
    case 'secret':
      return (
        <SecretInput
          id={id}
          isSet={current?.set === true}
          placeholder={placeholder}
          onSave={onSave}
        />
      );
    case 'number':
      return (
        <SavedInput
          id={id}
          type="number"
          trailing
          placeholder={placeholder}
          min={setting.min}
          max={setting.max}
          invalid={(text) => {
            const problem = settingProblem(setting, text);
            return problem ? problemText(problem) : undefined;
          }}
          value={value === undefined ? '' : String(value)}
          onSave={(text) => onSave(text === '' ? null : Number(text))}
        />
      );
    default:
      return (
        <SavedInput
          id={id}
          trailing
          placeholder={placeholder}
          invalid={(text) => {
            const problem = settingProblem(setting, text);
            return problem ? problemText(problem) : undefined;
          }}
          value={typeof value === 'string' ? value : ''}
          onSave={(text) => onSave(text === '' ? null : text)}
        />
      );
  }
}

/** A secret never comes back from the daemon: the field only replaces it. */
function SecretInput({
  id,
  isSet,
  placeholder,
  onSave,
}: {
  id: string;
  isSet: boolean;
  placeholder?: string;
  onSave: (value: unknown) => Promise<unknown>;
}) {
  const [draft, setDraft] = useState('');
  const { state, run } = useSaveState();
  const submit = () =>
    run(async () => {
      await onSave(draft);
      setDraft('');
    });

  return (
    <div className="flex items-center gap-2">
      <Input
        id={id}
        type="password"
        autoComplete="off"
        className="h-7 w-52 font-mono text-xs"
        placeholder={
          isSet
            ? m['settings.plugins.settings.secret_set']()
            : placeholder || m['settings.plugins.settings.secret_unset']()
        }
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && draft) void submit();
        }}
      />
      <SaveStatus state={state} />
      {(draft || state !== 'idle') && (
        <SaveButton variant="outline" state={state} onClick={submit} />
      )}
    </div>
  );
}
