import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import { type ContentProject, errorMessage } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { ProvisionProgressView } from '@/features/shared/entry/components';
import { m } from '@/paraglide/messages.js';
import { useInstallModpack } from '@/queries';
import { contentQueries } from '@/queries/content';
import { useInstances } from '@/queries/instance';
import { useJobDisplay } from '@/queries/jobs';
import { useServers } from '@/queries/server';

/**
 * Installing a modpack, which is unlike installing any other content: a pack
 * pins its own loader and game version, so the common case *creates* the entry
 * it wants rather than going into one that exists. Choosing an existing entry
 * is offered too, limited to the ones the chosen version names; the daemon
 * still decides.
 */
type Mode = 'instance' | 'server' | 'existing';

export function ModpackInstallDialog({
  project,
  pinnedVersion,
  open,
  onOpenChange,
}: {
  project: ContentProject;
  pinnedVersion?: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [mode, setMode] = useState<Mode>('instance');
  const [name, setName] = useState('');
  const [entry, setEntry] = useState('');
  const [eula, setEula] = useState(false);
  const [versionId, setVersionId] = useState('');
  const servers = useServers();
  const instances = useInstances();
  const published = useQuery({
    ...contentQueries.versions({ source: project.source, project: project.id }),
    enabled: open,
  });
  const install = useInstallModpack(mode === 'server' ? 'server' : 'instance');
  useJobDisplay(install.job, open);

  useEffect(() => {
    if (!open) return;
    setMode('instance');
    setName('');
    setEntry('');
    setEula(false);
    setVersionId('');
  }, [open]);

  const versions = published.data ?? [];
  const version =
    versions.find((v) => v.id === versionId) ??
    versions.find(
      (v) => v.id === pinnedVersion || v.versionNumber === pinnedVersion,
    ) ??
    versions.find((v) => v.channel === 'release') ??
    versions[0];
  const versionOptions = versions.map((v) => ({
    value: v.id,
    label: [
      v.versionNumber,
      v.gameVersions.join(', '),
      v.channel === 'release' ? '' : v.channel,
    ]
      .filter(Boolean)
      .join(' · '),
  }));
  // A source has no loader name for a vanilla pack, so a vanilla entry is left to the daemon.
  const takes = (e: { flavor: string; gameVersion: string }) =>
    !version ||
    (version.gameVersions.includes(e.gameVersion) &&
      (e.flavor === 'vanilla' || version.loaders.includes(e.flavor)));

  const entries = [
    ...(instances.data ?? []).map((i) => ({ ...i, kind: 'instance' as const })),
    ...(servers.data ?? []).map((s) => ({ ...s, kind: 'server' as const })),
  ];
  const modes = [
    { value: 'instance', label: m['instance.new']() },
    { value: 'server', label: m['server.new']() },
    { value: 'existing', label: m['content.modpack.target.existing']() },
  ];
  const entryOptions = entries.map((e) => ({
    value: e.id,
    label: `${e.name} · ${e.flavor} ${e.gameVersion}`,
    disabled: !takes(e),
  }));
  const target = entries.find((e) => e.id === entry && takes(e));
  // Installing into an existing entry follows that entry's own side, so the
  // server/instance choice is the target's, not the radio's.
  const kind = mode === 'existing' ? (target?.kind ?? 'instance') : mode;
  const ready = mode !== 'existing' ? kind !== 'server' || eula : !!target;

  async function run() {
    try {
      const result = await install.mutateAsync({
        pack: {
          source: project.source,
          project: project.id,
          version: version?.id ?? pinnedVersion ?? '',
        },
        target:
          mode === 'existing'
            ? { mode: 'existing', entry }
            : { mode: 'create', name: name.trim() },
        eula,
      });
      toast.success(
        m['content.modpack.install_into']({ name: result.entryName }),
      );
      onOpenChange(false);
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      size="lg"
      title={m['content.install_title']({ name: project.title })}
      description={m['content.modpack.target.label']()}
    >
      {install.progress ? (
        <ProvisionProgressView
          progress={install.progress}
          className="min-h-72"
        />
      ) : (
        <DialogBody className="space-y-4">
          <Field>
            <FieldLabel>{m['app.label.version']()}</FieldLabel>
            <Select
              items={versionOptions}
              value={version?.id ?? ''}
              onValueChange={(value) => setVersionId(value ?? '')}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {versionOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>

          <Field>
            <FieldLabel>{m['content.modpack.target.label']()}</FieldLabel>
            <Select
              items={modes}
              value={mode}
              onValueChange={(v) => setMode(v as Mode)}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {modes.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>

          {mode === 'existing' ? (
            <Field>
              <FieldLabel>{m['content.modpack.target.existing']()}</FieldLabel>
              <Select
                items={entryOptions}
                value={entry}
                onValueChange={(value) => setEntry(value ?? '')}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {entryOptions.map((option) => (
                    <SelectItem
                      key={option.value}
                      value={option.value}
                      disabled={option.disabled}
                    >
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {version && (
                <FieldDescription>
                  {m['content.modpack.target.needs']({
                    requirement: [
                      version.loaders.join(', '),
                      version.gameVersions.join(', '),
                    ]
                      .filter(Boolean)
                      .join(' '),
                  })}
                </FieldDescription>
              )}
            </Field>
          ) : (
            <Field>
              <FieldLabel>{m['app.label.name']()}</FieldLabel>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={m['content.modpack.name_placeholder']()}
              />
            </Field>
          )}

          {mode === 'server' && (
            <label
              htmlFor="modpack-eula"
              className="flex cursor-pointer items-start gap-2 text-xs"
            >
              <Checkbox
                id="modpack-eula"
                checked={eula}
                onCheckedChange={(v) => setEula(v === true)}
              />
              <span>{m['app.validation.eula']()}</span>
            </label>
          )}
        </DialogBody>
      )}

      <DialogFooter>
        <Button
          variant="ghost"
          onClick={() => onOpenChange(false)}
          disabled={install.isPending}
        >
          {m['app.action.cancel']()}
        </Button>
        <Button onClick={run} disabled={!ready || install.isPending}>
          {m['app.action.install']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}
