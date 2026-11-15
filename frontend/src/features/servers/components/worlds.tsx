import { GlobeIcon, UploadSimpleIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import { dialog } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Empty } from '@/components/empty';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { WorldRow } from '@/features/instances/components';
import { fileName } from '@/lib/path';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { serverMutations, serverQueries } from '@/queries/server';

/**
 * The worlds a server holds, with the one it loads marked, and the import of
 * a `.mcworld`. The daemon refuses an import while the server runs, so the
 * action says so rather than failing.
 */
export function ServerWorlds({
  id,
  running,
  activeWorld,
}: {
  id: string;
  running: boolean;
  /** The `level-name` in effect; the daemon lists the active world first. */
  activeWorld?: string;
}) {
  const worlds = useQuery(serverQueries.worlds(id));
  const importWorld = useJobMutation(serverMutations.importWorld(id));
  const [picked, setPicked] = useState<string | null>(null);
  const [use, setUse] = useState(true);

  const choose = async () => {
    const path = await dialog.pickWorldFile();
    if (!path) return;
    setUse(true);
    setPicked(path);
  };

  const submit = async () => {
    if (!picked) return;
    try {
      await importWorld.mutateAsync({ path: picked, use });
      setPicked(null);
    } catch {
      // The job store reports the failure; the dialog stays open to retry.
    }
  };

  const list = worlds.data ?? [];
  const pending = importWorld.isPending;

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-3">
        <h3 className="flex-1 text-xs font-semibold tracking-wide text-muted-foreground uppercase">
          {m['server.worlds.title']()}
        </h3>
        <Button
          size="sm"
          variant="outline"
          data-icon="inline-start"
          disabled={running || pending}
          title={running ? m['server.worlds.stop_first']() : undefined}
          onClick={choose}
        >
          <UploadSimpleIcon weight="bold" />
          {m['server.worlds.import']()}
        </Button>
      </div>

      {worlds.isPending ? (
        <Bone className="h-12" />
      ) : list.length === 0 ? (
        <Empty icon={GlobeIcon} className="py-8">
          {m['server.worlds.none']()}
        </Empty>
      ) : (
        <div className="divide-y divide-border border border-border">
          {list.map((world, at) => (
            <WorldRow
              key={world.folder}
              world={world}
              play={
                (activeWorld ? world.folder === activeWorld : at === 0) && (
                  <Badge variant="secondary">
                    {m['server.worlds.active']()}
                  </Badge>
                )
              }
            />
          ))}
        </div>
      )}

      <AppDialog
        open={picked !== null}
        onOpenChange={(next) => !pending && !next && setPicked(null)}
        title={m['server.worlds.import']()}
        description={picked ? fileName(picked) : undefined}
      >
        <DialogBody>
          <label
            htmlFor="world-use"
            className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground"
          >
            <Checkbox
              id="world-use"
              checked={use}
              onCheckedChange={(c) => setUse(c === true)}
              disabled={pending}
            />
            {m['server.worlds.use']()}
          </label>
        </DialogBody>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => setPicked(null)}
            disabled={pending}
          >
            {m['app.action.cancel']()}
          </Button>
          <Button onClick={submit} disabled={pending}>
            {pending
              ? m['app.status.preparing']()
              : m['server.worlds.import']()}
          </Button>
        </DialogFooter>
      </AppDialog>
    </section>
  );
}
