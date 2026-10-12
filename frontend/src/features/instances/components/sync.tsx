import { useMutation, useQuery } from '@tanstack/react-query';

import {
  SettingGroup,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Bone } from '@/components/skeleton';
import { StatusDot } from '@/components/ui/status-dot';
import { Switch } from '@/components/ui/switch';
import { RowList } from '@/features/settings/components';
import { SYNC_UNITS, stateLabel, stateTone, unitLabel } from '@/lib/sync';
import { m } from '@/paraglide/messages.js';
import { syncMutations, syncQueries } from '@/queries/sync';

/**
 * What this instance takes from the shared settings, and which game settings it
 * keeps to itself. Nothing here moves files: the next launch applies it.
 */
export function InstanceSyncField({ id }: { id: string }) {
  const status = useQuery(syncQueries.status());
  const setUnit = useMutation(syncMutations.setInstanceUnit(id));
  const setKeys = useMutation(syncMutations.setInstanceUnsynced(id));

  if (status.isPending) return <Bone className="h-24" />;

  const mine = status.data?.find((instance) => instance.id === id);
  const units = (mine?.units ?? []).filter((unit) => unit.state !== 'off');

  if (units.length === 0) {
    return (
      <SettingGroup
        legend={m['instance.sync.title']()}
        description={m['instance.sync.nothing_shared']()}
      >
        {null}
      </SettingGroup>
    );
  }

  return (
    <SettingGroup
      legend={m['instance.sync.title']()}
      description={m['instance.sync.description']()}
    >
      <SettingList>
        {SYNC_UNITS.map((unit) => {
          const state = units.find((entry) => entry.unit === unit)?.state;
          if (!state) return null;
          return (
            <SettingRow
              key={unit}
              htmlFor={`sync-${unit}`}
              label={
                <>
                  <StatusDot tone={stateTone[state]} />
                  {unitLabel[unit]()}
                </>
              }
              description={stateLabel[state]()}
            >
              <Switch
                id={`sync-${unit}`}
                size="sm"
                checked={state !== 'overridden'}
                disabled={setUnit.isPending || state === 'unsupported'}
                onCheckedChange={(checked) =>
                  setUnit.mutate({ unit, shared: checked === true })
                }
              />
            </SettingRow>
          );
        })}
      </SettingList>

      <RowList
        label={m['instance.sync.keys.label']()}
        description={m['instance.sync.keys.hint']()}
        placeholder={m['instance.sync.keys.placeholder']()}
        values={mine?.unsynced ?? []}
        pending={setKeys.isPending}
        onChange={(keys) => setKeys.mutate(keys)}
      />
    </SettingGroup>
  );
}
