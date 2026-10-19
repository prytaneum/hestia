import { useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { system } from '@/api';
// Not the dialogs barrel: its launch dialog imports the entry components back.
import { ExportInstanceDialog } from '@/features/instances/dialogs/export';
import { notify } from '@/lib/notify';
import { instanceQueries } from '@/queries/instance';
import { usePinned } from '@/queries/pinned';
import { serverQueries } from '@/queries/server';
import { RemoveEntryDialog } from '../dialogs/remove';

export interface EntryActionTarget {
  kind: 'instance' | 'server';
  id: string;
  name: string;
  running: boolean;
}

/**
 * The actions every entry menu offers, so the right-click menu and the detail
 * page's overflow stay one list. `dialogs` must be rendered by the caller,
 * outside any link: a portal still bubbles React events.
 */
export function useEntryActions(entry: EntryActionTarget) {
  const { kind, id, name, running } = entry;
  const queryClient = useQueryClient();
  const pins = usePinned();
  const [removing, setRemoving] = useState(false);
  const [exporting, setExporting] = useState(false);

  const openFolder = () => {
    const info =
      kind === 'server'
        ? queryClient.fetchQuery(serverQueries.info(id))
        : queryClient.fetchQuery(instanceQueries.info(id));
    info
      .then(({ entryDir }) => system.openPath(entryDir))
      .catch((error: Error) =>
        notify.failure(error, { id: `open-folder:${kind}:${id}` }),
      );
  };

  return {
    openFolder,
    exportArchive: kind === 'instance' ? () => setExporting(true) : undefined,
    pinned: pins.isPinned({ kind, id }),
    pinReady: pins.ready,
    togglePin: () => pins.toggle({ kind, id }),
    remove: () => setRemoving(true),
    dialogs: (
      <>
        <RemoveEntryDialog
          kind={kind}
          id={id}
          name={name}
          running={running}
          open={removing}
          onOpenChange={setRemoving}
        />
        {kind === 'instance' && (
          <ExportInstanceDialog
            id={id}
            name={name}
            open={exporting}
            onOpenChange={setExporting}
          />
        )}
      </>
    ),
  };
}
