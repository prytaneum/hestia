import { ArrowsClockwiseIcon } from '@phosphor-icons/react';
import type { UseQueryOptions } from '@tanstack/react-query';
import { useState } from 'react';

import type { GameVersion } from '@/api';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { m } from '@/paraglide/messages.js';
import type { JobEntryKind } from '@/queries';
import { ChangeVersionDialog, type UpdateHandle } from '../dialogs';
import { useDraft } from '../hooks';
import { SettingsSection } from './layout';

export interface GeneralSectionProps {
  kind: JobEntryKind;
  entry: { name: string; flavor: string; gameVersion: string };
  running: boolean;
  rename: { mutate: (name: string) => void; isPending: boolean };
  update: UpdateHandle;
  // biome-ignore lint/suspicious/noExplicitAny: the query factories' option types differ per domain.
  versionsQuery: UseQueryOptions<GameVersion[], any, GameVersion[], any>;
}

/** Identity: the entry's name and the game version it runs. */
export function GeneralSection({
  kind,
  entry,
  running,
  rename,
  update,
  versionsQuery,
}: GeneralSectionProps) {
  const [name, setName] = useDraft(entry.name, entry.name);
  const [changing, setChanging] = useState(false);
  const trimmed = name.trim();

  return (
    <SettingsSection
      title={m['entry.settings.section.general']()}
      description={running ? m['entry.settings.stop_to_change']() : undefined}
    >
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="entry-name">
            {kind === 'server'
              ? m['entry.settings.server_name']()
              : m['entry.settings.instance_name']()}
          </FieldLabel>
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (trimmed && trimmed !== entry.name) rename.mutate(trimmed);
            }}
          >
            <Input
              id="entry-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={running}
            />
            <Button
              type="submit"
              variant="outline"
              disabled={
                running ||
                rename.isPending ||
                !trimmed ||
                trimmed === entry.name
              }
            >
              {m['app.action.apply']()}
            </Button>
          </form>
        </Field>

        <Field>
          <FieldLabel>{m['app.label.version']()}</FieldLabel>
          <div className="flex items-center gap-3">
            <span className="font-mono text-xs">
              {entry.flavor} {entry.gameVersion}
            </span>
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
          </div>
          {kind === 'server' && (
            <FieldDescription>
              {m['entry.settings.version_backup_hint']()}
            </FieldDescription>
          )}
        </Field>
      </FieldGroup>

      <ChangeVersionDialog
        name={entry.name}
        gameVersion={entry.gameVersion}
        versionsQuery={versionsQuery}
        update={update}
        open={changing}
        onOpenChange={setChanging}
      />
    </SettingsSection>
  );
}
