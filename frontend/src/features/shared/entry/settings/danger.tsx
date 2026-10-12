import { TrashIcon } from '@phosphor-icons/react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';

import { SettingGroup } from '@/components/setting-controls';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
} from '@/components/ui/field';
import { m } from '@/paraglide/messages.js';
import type { JobEntryKind } from '@/queries';

export interface RemoveHandle {
  mutate: (v: undefined, o: { onSuccess: () => void }) => void;
}

/** Irreversible actions, kept apart from everything that can be undone. */
export function DangerSettings({
  kind,
  name,
  running,
  remove,
}: {
  kind: JobEntryKind;
  name: string;
  running: boolean;
  remove: RemoveHandle;
}) {
  const navigate = useNavigate();
  const server = kind === 'server';
  const label = server
    ? m['entry.settings.remove.server']()
    : m['entry.settings.remove.instance']();

  return (
    <SettingGroup legend={m['entry.settings.section.danger']()}>
      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel className="font-normal">{label}</FieldLabel>
          <FieldDescription>
            {m['entry.settings.remove.summary']()}
          </FieldDescription>
        </FieldContent>
        <ConfirmDialog
          trigger={
            <Button
              variant="destructive"
              size="sm"
              data-icon="inline-start"
              disabled={running}
            >
              <TrashIcon />
              {m['app.action.remove']()}
            </Button>
          }
          title={
            server
              ? m['entry.settings.remove.server_title']()
              : m['entry.settings.remove.instance_title']()
          }
          description={m['entry.settings.remove.description']({ name })}
          destructive
          confirmLabel={label}
          onConfirm={() =>
            remove.mutate(undefined, {
              onSuccess: () => {
                toast.success(m['app.toast.removed']({ name }));
                navigate({ to: server ? '/servers' : '/instances' });
              },
            })
          }
        />
      </Field>
    </SettingGroup>
  );
}
