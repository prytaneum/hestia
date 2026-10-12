import { ArrowsClockwiseIcon } from '@phosphor-icons/react';
import type { UseQueryOptions } from '@tanstack/react-query';
import { useState } from 'react';

import type { GameVersion } from '@/api';
import { SavedInput, SettingGroup } from '@/components/setting-controls';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
} from '@/components/ui/field';
import { m } from '@/paraglide/messages.js';
import type { JobEntryKind } from '@/queries';
import { ChangeVersionDialog, type UpdateHandle } from '../dialogs';

export interface GeneralSettingsProps {
  kind: JobEntryKind;
  entry: { name: string; flavor: string; gameVersion: string };
  running: boolean;
  rename: (name: string) => void;
  update: UpdateHandle;
  // biome-ignore lint/suspicious/noExplicitAny: the query factories' option types differ per domain.
  versionsQuery: UseQueryOptions<GameVersion[], any, GameVersion[], any>;
}

/** Identity: the entry's name and the game version it runs. */
export function GeneralSettings({
  kind,
  entry,
  running,
  rename,
  update,
  versionsQuery,
}: GeneralSettingsProps) {
  const [changing, setChanging] = useState(false);

  return (
    <SettingGroup
      legend={m['entry.settings.section.general']()}
      description={running ? m['entry.settings.stop_to_change']() : undefined}
    >
      <Field>
        <FieldLabel htmlFor="entry-name">
          {kind === 'server'
            ? m['entry.settings.server_name']()
            : m['entry.settings.instance_name']()}
        </FieldLabel>
        <SavedInput
          id="entry-name"
          value={entry.name}
          disabled={running}
          onSave={(name) => {
            if (name) rename(name);
          }}
        />
      </Field>

      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel className="font-normal">
            {m['app.label.version']()}
          </FieldLabel>
          <FieldDescription>
            <span className="font-mono">
              {entry.flavor} {entry.gameVersion}
            </span>
            {kind === 'server' &&
              ` — ${m['entry.settings.version_backup_hint']()}`}
          </FieldDescription>
        </FieldContent>
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
      </Field>

      <ChangeVersionDialog
        name={entry.name}
        gameVersion={entry.gameVersion}
        versionsQuery={versionsQuery}
        update={update}
        open={changing}
        onOpenChange={setChanging}
      />
    </SettingGroup>
  );
}
