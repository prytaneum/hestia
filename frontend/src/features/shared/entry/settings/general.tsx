import { ArrowsClockwiseIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import type { EntryKind } from '@/api';
import {
  SavedInput,
  SettingGroup,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Button } from '@/components/ui/button';
import { m } from '@/paraglide/messages.js';
import { ChangeVersionDialog, type UpdateHandle } from '../dialogs';

interface GeneralSettingsProps {
  kind: EntryKind;
  entry: { id: string; name: string; flavor: string; gameVersion: string };
  running: boolean;
  rename: (name: string) => void;
  update: UpdateHandle;
}

/** Identity: the entry's name and the game version it runs. */
export function GeneralSettings({
  kind,
  entry,
  running,
  rename,
  update,
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
        kind={kind}
        id={entry.id}
        name={entry.name}
        flavor={entry.flavor}
        gameVersion={entry.gameVersion}
        update={update}
        open={changing}
        onOpenChange={setChanging}
      />
    </SettingGroup>
  );
}
