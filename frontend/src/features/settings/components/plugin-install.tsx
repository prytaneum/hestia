import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';

import { dialog, errorMessage, type PluginInspection } from '@/api';
import { FileDropButton } from '@/components/file-drop-button';
import { Button } from '@/components/ui/button';
import { approvalDialog, describeApproval } from '@/features/settings/lib';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { pluginMutations } from '@/queries/plugin';

interface Candidate {
  path: string;
  inspection: PluginInspection;
}

/**
 * Install from a local package: inspect it, have the user approve exactly what
 * it asks for in a native dialog, then install against that grant's digest. A
 * grant too long for the dialog is read here first; a source other than a file
 * is another picker feeding `inspectPackage`.
 */
export function PluginInstall() {
  const inspect = useMutation(pluginMutations.inspect());
  const install = useJobMutation(pluginMutations.install());
  const [review, setReview] = useState<Candidate | null>(null);

  const approve = async ({ path, inspection }: Candidate) => {
    setReview(null);
    const { title, message } = approvalDialog(inspection);
    const approved = await dialog.ask(message, {
      title,
      okLabel: m['app.action.install'](),
      cancelLabel: m['app.action.cancel'](),
    });
    if (approved)
      install.mutate({
        id: inspection.manifest.id,
        path,
        approve: inspection.grantDigest,
      });
  };

  const inspectPackage = async (path: string) => {
    try {
      const inspection = await inspect.mutateAsync(path);
      const candidate = { path, inspection };
      if (approvalDialog(inspection).complete) await approve(candidate);
      else setReview(candidate);
    } catch (error) {
      notify.error(errorMessage(error));
    }
  };

  return (
    <div className="space-y-3">
      <FileDropButton
        title={m['settings.plugins.choose']()}
        hint={m['settings.plugins.choose_hint']()}
        accept={dialog.PLUGIN_EXTENSIONS}
        pick={dialog.pickPluginPackage}
        onFiles={([path]) => void inspectPackage(path)}
      />
      {review && (
        <Review
          candidate={review}
          onContinue={() => void approve(review)}
          onCancel={() => setReview(null)}
        />
      )}
    </div>
  );
}

function Review({
  candidate,
  onContinue,
  onCancel,
}: {
  candidate: Candidate;
  onContinue: () => void;
  onCancel: () => void;
}) {
  const { manifest } = candidate.inspection;
  const sections = describeApproval(candidate.inspection);

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
      {sections.map((section) => (
        <div key={section.heading}>
          <div className="text-xs font-medium">{section.heading}</div>
          <ul className="text-[11px] text-muted-foreground">
            {section.lines.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </div>
      ))}
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
