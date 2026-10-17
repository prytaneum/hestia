import { ArchiveIcon, PlusIcon, TrashIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { toast } from 'sonner';

import type { BackupInfo, BackupKind } from '@/api';
import { Empty } from '@/components/empty';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { TabToolbar } from '@/features/shared/entry/detail';
import { agoLabel, bytes } from '@/lib/format';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { serverMutations, serverQueries } from '@/queries/server';

function kindLabel(kind: BackupKind): string {
  switch (kind) {
    case 'manual':
      return m['domain.backup_kind.manual']();
    case 'scheduled':
      return m['domain.backup_kind.scheduled']();
    case 'update':
      return m['domain.backup_kind.update']();
  }
}

export function ServerBackupsTab({
  id,
  running,
}: {
  id: string;
  running: boolean;
}) {
  const backups = useQuery(serverQueries.backups(id));
  const create = useJobMutation(serverMutations.backup.create(id));
  const restore = useJobMutation(serverMutations.backup.restore(id));
  const remove = useMutation(serverMutations.backup.remove(id));

  const list = backups.data ?? [];

  return (
    <>
      <TabToolbar className="justify-end">
        <Button
          size="sm"
          variant="outline"
          data-icon="inline-start"
          disabled={create.isPending}
          onClick={() =>
            create.mutate(undefined, {
              onSuccess: () => toast.success(m['app.toast.saved']()),
            })
          }
        >
          <PlusIcon weight="bold" />
          {m['server.backup.create']()}
        </Button>
      </TabToolbar>

      {list.length === 0 ? (
        <Empty className="flex-1" icon={ArchiveIcon}>
          {m['server.backup.none']()}
        </Empty>
      ) : (
        <div className="divide-y divide-border border border-border">
          {list.map((backup: BackupInfo) => (
            <div
              key={backup.id}
              className="flex items-center gap-3 px-3 py-2.5"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-sm">
                    {agoLabel(backup.createdUnix)}
                  </span>
                  <Badge variant="secondary" className="shrink-0 capitalize">
                    {kindLabel(backup.kind)}
                  </Badge>
                </div>
                <div className="font-mono text-[11px] text-muted-foreground">
                  {bytes(backup.size)}
                </div>
              </div>
              <ConfirmDialog
                trigger={
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={running || restore.isPending}
                  >
                    {m['app.action.restore']()}
                  </Button>
                }
                title={m['server.backup.restore.title']()}
                description={m['server.backup.restore.description']({
                  when: agoLabel(backup.createdUnix),
                })}
                confirmLabel={m['app.action.restore']()}
                onConfirm={() =>
                  restore.mutate(backup.id, {
                    onSuccess: () => toast.success(m['app.toast.saved']()),
                  })
                }
              />
              <ConfirmDialog
                trigger={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={m['server.backup.delete.aria']()}
                  >
                    <TrashIcon className="size-4" />
                  </Button>
                }
                title={m['server.backup.delete.title']()}
                description={m['server.backup.delete.description']({
                  when: agoLabel(backup.createdUnix),
                })}
                destructive
                confirmLabel={m['app.action.delete']()}
                onConfirm={() => remove.mutate(backup.id)}
              />
            </div>
          ))}
        </div>
      )}
    </>
  );
}
