import { LinkIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';

import { dialog, errorMessage, type PluginInspection } from '@/api';
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
import { approvalDialog, describeApproval } from '@/features/settings/lib';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { pluginMutations } from '@/queries/plugin';

import { GrantList } from './plugin-grant';

interface Candidate {
  source: PluginPackage;
  inspection: PluginInspection;
}

/** A link's package was downloaded by the inspect; install that file, not the link again. */
function installTarget({ source, inspection }: Candidate): PluginInstallTarget {
  return 'path' in source
    ? { path: source.path }
    : { staged: inspection.staged ?? '' };
}

/**
 * Install from a local package or an https link: inspect it, have the user
 * approve exactly what it asks for in a native dialog, then install against
 * that grant's digest. A grant too long for the dialog is reviewed on the page
 * first (`review`).
 */
export function usePluginInstall() {
  const inspect = useMutation(pluginMutations.inspect());
  const install = useJobMutation(pluginMutations.install());
  const [review, setReview] = useState<Candidate | null>(null);

  const approve = async (candidate: Candidate) => {
    setReview(null);
    const { inspection } = candidate;
    const { title, message } = approvalDialog(inspection);
    const approved = await dialog.ask(message, {
      title,
      okLabel: m['app.action.install'](),
      cancelLabel: m['app.action.cancel'](),
    });
    if (approved)
      install.mutate({
        id: inspection.manifest.id,
        approve: inspection.grantDigest,
        ...installTarget(candidate),
      });
  };

  const start = async (source: PluginPackage) => {
    try {
      const inspection = await inspect.mutateAsync(source);
      const candidate = { source, inspection };
      if (approvalDialog(inspection).complete) await approve(candidate);
      else setReview(candidate);
    } catch (error) {
      notify.error(errorMessage(error));
    }
  };

  return {
    start,
    review,
    approve,
    dismiss: () => setReview(null),
    busy: inspect.isPending || install.isPending,
  };
}

/** The link variant of install, tucked into a popover beside the file button. */
export function InstallFromLink({
  busy,
  onSubmit,
}: {
  busy: boolean;
  onSubmit: (url: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState('');
  const link = url.trim();

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button variant="outline" size="sm" disabled={busy}>
            <LinkIcon />
            {m['settings.plugins.install_link']()}
          </Button>
        }
      />
      <PopoverContent align="end" className="w-80">
        <PopoverHeader>
          <PopoverTitle>{m['settings.plugins.link_title']()}</PopoverTitle>
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
            onSubmit(link);
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
