import { revalidateLogic } from '@tanstack/react-form';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { z } from 'zod';

import type { SyncUnit } from '@/api';
import { AppDialog, DialogFooter } from '@/components/dialog';
import { useAppForm } from '@/components/form';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
import { Switch } from '@/components/ui/switch';
import { Setting } from '@/features/settings/components';
import { SYNC_UNITS, unitHint, unitLabel } from '@/lib/sync';
import { m } from '@/paraglide/messages.js';
import { syncMutations, syncQueries } from '@/queries/sync';

/**
 * What the instances share. Turning one on has to start the shared copy from
 * someone's, so it asks which instance rather than letting the first launch
 * decide.
 */
export function SyncSettings() {
  const config = useQuery(syncQueries.config());
  const disable = useMutation(syncMutations.disable());
  const [picking, setPicking] = useState<SyncUnit | null>(null);

  if (config.isPending) return <Bone className="h-40" />;

  const units = config.data?.units ?? [];
  const optionsShared = units.some(
    (unit) => unit.unit === 'options' && unit.enabled,
  );

  return (
    <>
      <Setting id="sync-units">
        <div className="divide-y divide-border border border-border">
          {SYNC_UNITS.map((unit) => {
            const entry = units.find((candidate) => candidate.unit === unit);
            return (
              <label
                key={unit}
                htmlFor={`sync-${unit}`}
                className="flex cursor-pointer items-center gap-3 px-3 py-2"
              >
                <div className="min-w-0 flex-1">
                  <div className="text-xs">{unitLabel[unit]()}</div>
                  <div className="text-[11px] text-muted-foreground">
                    {entry?.enabled && entry.seededFrom
                      ? m['settings.sync.seeded_from']({
                          instance: entry.seededFrom,
                        })
                      : unitHint[unit]()}
                  </div>
                </div>
                <Switch
                  id={`sync-${unit}`}
                  size="sm"
                  checked={entry?.enabled ?? false}
                  disabled={disable.isPending}
                  onCheckedChange={(checked) =>
                    checked === true ? setPicking(unit) : disable.mutate(unit)
                  }
                />
              </label>
            );
          })}
        </div>
      </Setting>

      {optionsShared && <SharedOptions />}

      <SourcePicker unit={picking} onClose={() => setPicking(null)} />
    </>
  );
}

function SourcePicker({
  unit,
  onClose,
}: {
  unit: SyncUnit | null;
  onClose: () => void;
}) {
  const sources = useQuery({
    ...syncQueries.sources(unit ?? 'options'),
    enabled: unit !== null,
  });
  const enable = useMutation(syncMutations.enable());

  const candidates = (sources.data ?? []).filter((source) => source.present);

  const form = useAppForm({
    defaultValues: { source: '' },
    validationLogic: revalidateLogic(),
    validators: {
      onDynamic: z.object({
        source: z.string().min(1, m['settings.sync.source.required']()),
      }),
    },
    onSubmit: async ({ value, formApi }) => {
      if (unit === null) return;
      await enable.mutateAsync({ unit, source: value.source });
      formApi.reset();
      onClose();
    },
  });

  const startEmpty = async () => {
    if (unit === null) return;
    await enable.mutateAsync({ unit, source: '' });
    onClose();
  };

  return (
    <AppDialog
      open={unit !== null}
      onOpenChange={(open) => !open && onClose()}
      title={m['settings.sync.source.title']()}
      description={m['settings.sync.source.description']()}
    >
      {sources.isPending ? (
        <Bone className="h-24" />
      ) : candidates.length === 0 ? (
        <div className="flex flex-col gap-3">
          <FieldDescription>
            {m['settings.sync.source.none']()}
          </FieldDescription>
          <DialogFooter>
            <Button onClick={startEmpty} disabled={enable.isPending}>
              {m['settings.sync.source.empty_action']()}
            </Button>
          </DialogFooter>
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            event.stopPropagation();
            form.handleSubmit();
          }}
        >
          <form.AppField name="source">
            {(field) => (
              <field.SelectField
                label={m['settings.sync.source.label']()}
                placeholder={m['settings.sync.source.placeholder']()}
                options={candidates.map((source) => ({
                  value: source.name,
                  label: source.name,
                }))}
              />
            )}
          </form.AppField>
          <DialogFooter className="mt-4">
            <form.AppForm>
              <form.SubmitButton>
                {m['settings.sync.source.action']()}
              </form.SubmitButton>
            </form.AppForm>
          </DialogFooter>
        </form>
      )}
    </AppDialog>
  );
}

/** The shared `options.txt`, key by key — the only way to read those values
 * without opening the game. */
function SharedOptions() {
  const config = useQuery(syncQueries.config());
  const options = useQuery(syncQueries.options());
  const setOption = useMutation(syncMutations.setOption());
  const setUnsynced = useMutation(syncMutations.setUnsynced());

  const unsynced = config.data?.unsynced ?? [];

  const form = useAppForm({
    defaultValues: { key: '', value: '' },
    validationLogic: revalidateLogic(),
    validators: {
      onDynamic: z.object({
        key: z.string().min(1, m['settings.sync.options.key_required']()),
        value: z.string(),
      }),
    },
    onSubmit: async ({ value, formApi }) => {
      await setOption.mutateAsync(value);
      formApi.reset();
    },
  });

  const share = (key: string, shared: boolean) =>
    setUnsynced.mutate(
      shared ? unsynced.filter((other) => other !== key) : [...unsynced, key],
    );

  return (
    <Setting id="sync-options">
      <Field>
        <FieldLabel>{m['settings.sync.options.label']()}</FieldLabel>
        <FieldDescription>{m['settings.sync.options.hint']()}</FieldDescription>

        {options.isPending ? (
          <Bone className="h-24" />
        ) : options.data?.length === 0 ? (
          <FieldDescription>
            {m['settings.sync.options.empty']()}
          </FieldDescription>
        ) : (
          <div className="divide-y divide-border border border-border">
            {options.data?.map((option) => (
              <div
                key={option.key}
                className="flex items-center gap-3 px-3 py-1.5 text-xs"
              >
                <span className="min-w-0 flex-1 truncate font-mono">
                  {option.key}
                </span>
                <span className="truncate font-mono text-muted-foreground">
                  {option.value}
                </span>
                <Switch
                  size="sm"
                  aria-label={m['settings.sync.options.share_key']({
                    key: option.key,
                  })}
                  checked={option.synced}
                  disabled={setUnsynced.isPending}
                  onCheckedChange={(checked) =>
                    share(option.key, checked === true)
                  }
                />
              </div>
            ))}
          </div>
        )}

        <form
          className="flex items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            event.stopPropagation();
            form.handleSubmit();
          }}
        >
          <form.AppField name="key">
            {(field) => (
              <field.TextField
                label={m['settings.sync.options.key_label']()}
                placeholder="guiScale"
                className="flex-1"
                inputClassName="font-mono"
              />
            )}
          </form.AppField>
          <form.AppField name="value">
            {(field) => (
              <field.TextField
                label={m['settings.sync.options.value_label']()}
                className="flex-1"
                inputClassName="font-mono"
              />
            )}
          </form.AppField>
          <form.AppForm>
            <form.SubmitButton>
              {m['settings.sync.options.action']()}
            </form.SubmitButton>
          </form.AppForm>
        </form>
      </Field>
    </Setting>
  );
}
