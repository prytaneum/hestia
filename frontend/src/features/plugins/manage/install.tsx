import { FileArrowUpIcon, PlusIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';

import {
  dialog,
  type OutdatedPlugin,
  type PluginInfo,
  type PluginInspection,
} from '@/api';
import type { PluginInstallTarget, PluginPackage } from '@/api/plugin';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/ui/popover';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { pluginMutations } from '@/queries/plugin';

import { approvalDialog, describeApproval } from './approval';
import { GrantList } from './grant';

interface Candidate {
  source: PluginPackage;
  inspection: PluginInspection;
  dev: boolean;
  /** The installed plugin this candidate updates, through `plugin.update`. */
  updates?: string;
}

/** A link's package was downloaded by the inspect; install that file, not the link again. */
function installTarget({
  source,
  inspection,
  dev,
}: Candidate): PluginInstallTarget {
  if ('path' in source)
    return dev ? { path: source.path, dev } : { path: source.path };
  return { staged: inspection.staged ?? '' };
}

/**
 * Install from a local package or an https link: inspect it, have the user
 * approve exactly what it asks for in a native dialog, then install against
 * that grant's digest. A grant too long for the dialog is reviewed on the page
 * first (`review`). An update takes the same road to the `plugin.update` job,
 * and is applied without asking when it adds nothing to the approved grant.
 *
 * `start` and `update` resolve once the job has settled; the job store reports
 * its outcome, so a failure is not rethrown here.
 */
export function usePluginInstall() {
  const inspect = useMutation(pluginMutations.inspect());
  const install = useJobMutation(pluginMutations.install());
  const update = useJobMutation(pluginMutations.update());
  const check = useMutation(pluginMutations.checkLink());
  const [review, setReview] = useState<Candidate | null>(null);

  const approve = async (candidate: Candidate) => {
    setReview(null);
    const { inspection } = candidate;
    const { title, message } = approvalDialog(inspection);
    const approved = await dialog.ask(message, {
      title,
      okLabel: candidate.updates
        ? m['settings.plugins.update']()
        : m['app.action.install'](),
      cancelLabel: m['app.action.cancel'](),
    });
    if (!approved) return;
    const job = candidate.updates
      ? update.mutateAsync({
          plugin: candidate.updates,
          approve: inspection.grantDigest,
        })
      : install.mutateAsync({
          id: inspection.manifest.id,
          approve: inspection.grantDigest,
          ...installTarget(candidate),
        });
    await job.catch(() => undefined);
  };

  const start = async (source: PluginPackage, dev = false) => {
    try {
      const inspection = await inspect.mutateAsync(source);
      const candidate = { source, inspection, dev };
      if (approvalDialog(inspection).complete) await approve(candidate);
      else setReview(candidate);
    } catch (error) {
      notify.fail(error);
    }
  };

  /** False when the update now waits on the page's review instead of having run. */
  const startUpdate = async (
    offer: OutdatedPlugin,
    link?: string,
  ): Promise<boolean> => {
    try {
      const source: PluginPackage = offer.repository
        ? { repo: offer.repository, plugin: offer.id }
        : { url: link ?? '' };
      const inspection = await inspect.mutateAsync(source);
      const candidate = { source, inspection, dev: false, updates: offer.id };
      if (describeApproval(inspection).length === 0)
        await update
          .mutateAsync({ plugin: offer.id, approve: inspection.grantDigest })
          .catch(() => undefined);
      else if (approvalDialog(inspection).complete) await approve(candidate);
      else {
        setReview(candidate);
        return false;
      }
    } catch (error) {
      notify.fail(error);
    }
    return true;
  };

  const checkLink = async (plugin: PluginInfo) => {
    try {
      const offer = await check.mutateAsync(plugin.manifest.id);
      if (offer) await startUpdate(offer, plugin.url);
      else
        notify.info(
          m['settings.plugins.up_to_date']({
            name: plugin.manifest.name || plugin.manifest.id,
          }),
        );
    } catch (error) {
      notify.fail(error);
    }
  };

  return {
    start,
    update: startUpdate,
    checkLink,
    review,
    approve,
    dismiss: () => setReview(null),
    busy:
      inspect.isPending ||
      install.isPending ||
      update.isPending ||
      check.isPending,
  };
}

export type PluginInstall = ReturnType<typeof usePluginInstall>;

/** The page's one way in for a package that is not in a repository: a link or a file. */
export function InstallMenu({ install }: { install: PluginInstall }) {
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState('');
  const link = url.trim();

  const pickFile = async () => {
    setOpen(false);
    const [path] = await dialog.pickPluginPackage();
    if (path) await install.start({ path });
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button variant="outline" size="sm" disabled={install.busy}>
            <PlusIcon />
            {m['plugins.manage.install.open']()}
          </Button>
        }
      />
      <PopoverContent align="end" className="w-80">
        <PopoverHeader>
          <PopoverTitle>{m['plugins.manage.install.title']()}</PopoverTitle>
          <PopoverDescription>
            {m['settings.plugins.link_hint']()}
          </PopoverDescription>
        </PopoverHeader>
        <form
          className="flex gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (!link) return;
            setOpen(false);
            setUrl('');
            void install.start({ url: link });
          }}
        >
          <Input
            type="url"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            placeholder="https://…/plugin.hplugin"
            aria-label={m['settings.plugins.link_title']()}
          />
          <Button type="submit" size="sm" disabled={!link}>
            {m['app.action.install']()}
          </Button>
        </form>
        <Button variant="outline" size="sm" onClick={() => void pickFile()}>
          <FileArrowUpIcon />
          {m['settings.plugins.install_file']()}
        </Button>
      </PopoverContent>
    </Popover>
  );
}

/** A grant too long for the native dialog, read on the page before it opens. */
export function InstallReview({
  inspection,
  onContinue,
  onCancel,
}: {
  inspection: PluginInspection;
  onContinue: () => void;
  onCancel: () => void;
}) {
  const { manifest } = inspection;

  return (
    <div className="space-y-3 border border-border p-3">
      <div>
        <div className="text-sm">
          {m['settings.plugins.review_title']({
            name: manifest.name || manifest.id,
            version: manifest.version,
          })}
        </div>
        <div className="text-[11px] text-muted-foreground">
          {m['settings.plugins.review_hint']()}
        </div>
      </div>
      <GrantList sections={describeApproval(inspection)} />
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onCancel}>
          {m['app.action.cancel']()}
        </Button>
        <Button size="sm" onClick={onContinue}>
          {m['settings.plugins.continue']()}
        </Button>
      </div>
    </div>
  );
}
