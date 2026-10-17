import { CaretLeftIcon, CaretRightIcon } from '@phosphor-icons/react';
import { revalidateLogic } from '@tanstack/react-form';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';

import type { ConfigEntry, Flavor } from '@/api';
import { dialog } from '@/api';
import { AppDialog, DialogFooter } from '@/components/dialog';
import { FileDropButton } from '@/components/file-drop-button';
import { useAppForm } from '@/components/form';
import { entryIcon } from '@/components/icons';
import { StepDots } from '@/components/step-dots';
import { Button } from '@/components/ui/button';
import { ProvisionProgressView } from '@/features/shared/entry/components';
import {
  bakeIcon,
  randomIconConfig,
} from '@/features/shared/entry/components/icon-editor/catalog';
import {
  createWizardDefaults,
  createWizardSchema,
  detailsStepSchema,
  flavorStepSchema,
  versionStepSchema,
  type WizardValues,
} from '@/features/shared/entry/lib';
import { memGb } from '@/lib/format';
import { toastWarnings } from '@/lib/warnings';
import { m } from '@/paraglide/messages.js';
import { configQueries, launcherDefaults } from '@/queries/config';
import { useMemoryRange } from '@/queries/host';
import { iconMutations } from '@/queries/icons';
import { instanceMutations, instanceQueries } from '@/queries/instance';
import { useJobDisplay, useJobMutation } from '@/queries/jobs';
import { serverMutations, serverQueries } from '@/queries/server';

import {
  FlavorOption,
  type Kind,
  STEP_HINTS,
  STEPS,
  type Step,
  StepForm,
} from './fields';
import { DetailsStep } from './steps/details';
import { VersionStep } from './steps/version';

/**
 * The New server / New instance wizard: flavor → version → details, wired to
 * the daemon's provider catalogue and the real create job. A server create
 * streams provisioning phases as a progress bar; an instance create is a quick
 * record write. On success the dialog closes and the list invalidates.
 */
export function CreateEntryDialog({
  kind,
  open,
  onOpenChange,
  onImport,
}: {
  kind: Kind;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Leave the wizard to import this archive — an instance can also arrive whole. */
  onImport?: (archive: string) => void;
}) {
  const [step, setStep] = useState<Step>('flavor');
  const [search, setSearch] = useState('');
  const [showSnapshots, setShowSnapshots] = useState(false);

  const serverFlavors = useQuery({
    ...serverQueries.flavors(),
    enabled: kind === 'server',
  });
  const instanceFlavors = useQuery({
    ...instanceQueries.flavors(),
    enabled: kind === 'instance',
  });
  const flavorsQuery = kind === 'server' ? serverFlavors : instanceFlavors;
  const flavors: Flavor[] = useMemo(
    () => flavorsQuery.data?.flavors ?? [],
    [flavorsQuery.data],
  );
  const rules = useMemo(
    () => flavorsQuery.data?.rules ?? [],
    [flavorsQuery.data],
  );

  const config = useQuery(configQueries.list());
  const defaultMemory = launcherDefaults(config.data).memory ?? '';
  const memoryRange = useMemoryRange();
  const defaultMemoryGb = Math.min(
    defaultMemory ? memGb(defaultMemory) : 4,
    memoryRange.max,
  );
  const defaultMemoryRef = useRef(defaultMemoryGb);
  defaultMemoryRef.current = defaultMemoryGb;

  const createServer = useJobMutation(serverMutations.create());
  const createInstance = useMutation(instanceMutations.create());
  const generateIcon = useMutation(iconMutations.generate());
  const creating = createServer.isPending || createInstance.isPending;
  const progress = createServer.progress;
  const job = createServer.job;

  useJobDisplay(job, open && creating);

  const applyRandomIcon = (entryId: string) => {
    const icon = randomIconConfig();
    bakeIcon(icon)
      .then((bytes) => generateIcon.mutate({ entryId, config: icon, bytes }))
      .catch(() => undefined);
  };

  const form = useAppForm({
    defaultValues: createWizardDefaults('', defaultMemoryGb),
    validationLogic: revalidateLogic(),
    validators: { onDynamic: createWizardSchema(kind, memoryRange, rules) },
    onSubmit: async ({ value }) => {
      // Inherit the launcher default when the slider is left on it: store no
      // per-entry override, so a later change to the global default cascades.
      const memoryEntries: ConfigEntry[] =
        defaultMemory !== '' &&
        value.details.memory === defaultMemoryRef.current
          ? []
          : [{ key: 'memory', value: `${value.details.memory}G` }];
      try {
        if (kind === 'server') {
          const created = await createServer.mutateAsync(
            serverParams(value, memoryEntries),
          );
          toast.success(m['app.toast.created']({ name: created.server.name }));
          toastWarnings(created.warnings);
          applyRandomIcon(created.server.id);
        } else {
          const instance = await createInstance.mutateAsync(
            instanceParams(value, memoryEntries),
          );
          toast.success(m['app.toast.created']({ name: instance.name }));
          applyRandomIcon(instance.id);
        }
        onOpenChange(false);
      } catch {
        // The global mutation error handler surfaces the toast; swallow here so
        // the dialog stays open without a second notification.
      }
    },
  });

  useEffect(() => {
    if (!open) return;
    setStep('flavor');
    setSearch('');
    setShowSnapshots(false);
    form.reset(createWizardDefaults('', defaultMemoryRef.current));
  }, [open, form]);

  const Icon = entryIcon(kind);
  const stepIndex = STEPS.indexOf(step);

  const nav = (
    <DialogFooter className="items-center">
      <StepDots steps={STEPS} active={stepIndex} className="mr-auto" />
      {stepIndex === 0 ? (
        <Button
          type="button"
          variant="outline"
          onClick={() => onOpenChange(false)}
        >
          {m['app.action.cancel']()}
        </Button>
      ) : (
        <Button
          type="button"
          variant="outline"
          onClick={() => setStep(STEPS[stepIndex - 1])}
          data-icon="inline-start"
        >
          <CaretLeftIcon />
          {m['app.action.back']()}
        </Button>
      )}
      {step === 'details' ? (
        <Button
          type="submit"
          className="bg-ember text-ember-foreground hover:bg-ember/90"
        >
          {kind === 'server'
            ? m['entry.create.server_title']()
            : m['entry.create.instance_title']()}
        </Button>
      ) : (
        <Button
          type="submit"
          data-icon="inline-end"
          className="bg-ember text-ember-foreground hover:bg-ember/90"
        >
          {m['app.action.next']()}
          <CaretRightIcon />
        </Button>
      )}
    </DialogFooter>
  );

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      size="xl"
      height="tall"
      icon={Icon}
      title={kind === 'server' ? m['server.new']() : m['instance.new']()}
      description={
        creating
          ? kind === 'server'
            ? m['entry.create.provisioning_server']()
            : m['entry.create.provisioning_instance']()
          : STEP_HINTS[step](kind)
      }
    >
      {creating ? (
        <ProvisionProgressView
          progress={progress}
          indeterminate={kind === 'instance'}
          className="min-h-0 flex-1"
        />
      ) : step === 'flavor' ? (
        <form.FormGroup
          name="flavor"
          validators={{ onDynamic: flavorStepSchema() }}
          onGroupSubmit={() => setStep('version')}
        >
          {(group) => (
            <StepForm onSubmit={group.handleSubmit} footer={nav}>
              <form.AppField name="flavor.flavor">
                {(field) => (
                  <div className="grid gap-2">
                    {flavors.map((f) => (
                      <FlavorOption
                        key={f.id}
                        flavor={f}
                        selected={field.state.value === f.id}
                        onSelect={() => {
                          field.handleChange(f.id);
                          form.setFieldValue('version.version', '');
                          form.setFieldValue('version.loaderVersion', '');
                        }}
                      />
                    ))}
                    {kind === 'instance' && onImport && (
                      <FileDropButton
                        title={m['instance.import.action']()}
                        hint={m['instance.import.formats']()}
                        accept={dialog.ARCHIVE_EXTENSIONS}
                        pick={dialog.pickInstanceArchive}
                        onFiles={([archive]) => {
                          onOpenChange(false);
                          onImport(archive);
                        }}
                      />
                    )}
                    {flavors.length === 0 && (
                      <p className="px-1 py-6 text-center text-xs text-muted-foreground">
                        {flavorsQuery.isPending
                          ? m['app.status.loading']()
                          : m['entry.create.no_flavors']()}
                      </p>
                    )}
                  </div>
                )}
              </form.AppField>
            </StepForm>
          )}
        </form.FormGroup>
      ) : step === 'version' ? (
        <form.FormGroup
          name="version"
          validators={{ onDynamic: versionStepSchema() }}
          onGroupSubmit={() => setStep('details')}
        >
          {(group) => (
            <StepForm onSubmit={group.handleSubmit} footer={nav}>
              <form.Subscribe selector={(s) => s.values.flavor.flavor}>
                {(flavor) => (
                  <VersionStep
                    form={form}
                    kind={kind}
                    flavor={flavor}
                    search={search}
                    onSearch={setSearch}
                    showSnapshots={showSnapshots}
                    onShowSnapshots={setShowSnapshots}
                  />
                )}
              </form.Subscribe>
            </StepForm>
          )}
        </form.FormGroup>
      ) : (
        <form.FormGroup
          name="details"
          validators={{
            onDynamic: detailsStepSchema(kind, memoryRange, rules),
          }}
          onGroupSubmit={() => form.handleSubmit()}
        >
          {(group) => (
            <StepForm onSubmit={group.handleSubmit} footer={nav}>
              <DetailsStep form={form} kind={kind} />
            </StepForm>
          )}
        </form.FormGroup>
      )}
    </AppDialog>
  );
}

/** Build the server create params from the wizard's collected values. */
function serverParams(value: WizardValues, memoryEntries: ConfigEntry[]) {
  const d = value.details;
  const config: ConfigEntry[] = [
    ...memoryEntries,
    { key: 'motd', value: d.motd },
    { key: 'gamemode', value: d.gamemode },
    { key: 'difficulty', value: d.difficulty },
    { key: 'max-players', value: d.maxPlayers },
    { key: 'hardcore', value: String(d.hardcore) },
    { key: 'online-mode', value: String(d.onlineMode) },
  ];
  const seed = d.seed.trim();
  if (seed) config.push({ key: 'level-seed', value: seed });
  return {
    ...instanceParams(value, memoryEntries),
    eula: true,
    port: d.port ? Number(d.port) : undefined,
    config,
  };
}

function instanceParams(value: WizardValues, memoryEntries: ConfigEntry[]) {
  const d = value.details;
  return {
    name: d.name || undefined,
    flavor: value.flavor.flavor,
    version: value.version.version,
    loaderVersion: value.version.loaderVersion || undefined,
    config: memoryEntries,
  };
}
