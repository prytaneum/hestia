import { TrashIcon } from '@phosphor-icons/react';
import { useNavigate } from '@tanstack/react-router';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { m } from '@/paraglide/messages.js';
import type { JobEntryKind } from '@/queries';
import { SettingsSection } from './layout';

export interface RemoveHandle {
  mutate: (v: undefined, o: { onSuccess: () => void }) => void;
}

/** Irreversible actions, kept apart from everything that can be undone. */
export function DangerSection({
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
    <SettingsSection
      title={m['entry.settings.section.danger']()}
      description={running ? m['entry.settings.stop_to_change']() : undefined}
    >
      <div className="flex items-center gap-4 border border-destructive/40 p-4">
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium">{label}</div>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {m['entry.settings.remove.summary']()}
          </p>
        </div>
        <ConfirmDialog
          trigger={
            <Button
              variant="destructive"
              size="sm"
              data-icon="inline-start"
              disabled={running}
            >
              <TrashIcon />
              {label}
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
      </div>
    </SettingsSection>
  );
}
