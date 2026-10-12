import { ArrowsClockwiseIcon } from '@phosphor-icons/react';
import type { UseQueryOptions } from '@tanstack/react-query';
import { useState } from 'react';

import type { GameVersion } from '@/api';
import {
  SavedInput,
  SettingGroup,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Button } from '@/components/ui/button';
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
      <SettingList>
        <SettingRow
          htmlFor="entry-name"
          label={
            kind === 'server'
              ? m['entry.settings.server_name']()
              : m['entry.settings.instance_name']()
          }
        >
          <SavedInput
            id="entry-name"
            trailing
            value={entry.name}
            disabled={running}
            onSave={(name) => {
              if (name) rename(name);
            }}
          />
        </SettingRow>

        <SettingRow
          label={
            <>
              {m['app.label.version']()}
              <span className="font-mono text-muted-foreground">
                {entry.flavor} {entry.gameVersion}
              </span>
            </>
          }
          description={
            kind === 'server'
              ? m['entry.settings.version_backup_hint']()
              : undefined
          }
        >
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
        </SettingRow>
      </SettingList>

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
