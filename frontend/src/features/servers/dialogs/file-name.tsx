import { useState } from 'react';

import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { Field, FieldError, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { nameIssue } from '@/features/servers/lib';
import { m } from '@/paraglide/messages.js';

export interface NamePrompt {
  title: string;
  /** What the confirm button says. */
  action: string;
  initial: string;
  /** Names already in the directory; one of them cannot be chosen. */
  taken: string[];
  /** A destination rather than a name: `/` is allowed and nothing is taken. */
  path?: boolean;
  onSubmit: (value: string) => void;
}

const ISSUES = {
  empty: m['server.files.name.empty'],
  separator: m['server.files.name.separator'],
  taken: m['server.files.name.taken'],
};

/**
 * The one dialog every file action that needs a name goes through — a new
 * file or folder, a rename, a move, an archive.
 */
export function FileNameDialog({
  prompt,
  onClose,
}: {
  prompt: NamePrompt | null;
  onClose: () => void;
}) {
  return (
    <AppDialog
      open={prompt !== null}
      onOpenChange={(open) => !open && onClose()}
      size="sm"
      title={prompt?.title}
    >
      {prompt && <NameForm prompt={prompt} onClose={onClose} />}
    </AppDialog>
  );
}

function NameForm({
  prompt,
  onClose,
}: {
  prompt: NamePrompt;
  onClose: () => void;
}) {
  const [value, setValue] = useState(prompt.initial);
  const trimmed = value.trim();
  const issue = prompt.path
    ? trimmed === ''
      ? 'empty'
      : null
    : nameIssue(value, prompt.taken);
  const unchanged = trimmed === prompt.initial;

  return (
    <form
      className="flex min-h-0 flex-1 flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (issue || unchanged) return;
        prompt.onSubmit(trimmed);
        onClose();
      }}
    >
      <DialogBody>
        <Field data-invalid={issue !== null && !unchanged}>
          <FieldLabel htmlFor="file-name">
            {prompt.path
              ? m['server.files.name.destination']()
              : m['app.label.name']()}
          </FieldLabel>
          <Input
            id="file-name"
            autoFocus
            className="font-mono"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            onFocus={(event) => {
              // Select the stem, so typing replaces the name and keeps the
              // extension.
              const dot = event.target.value.lastIndexOf('.');
              const slash = event.target.value.lastIndexOf('/') + 1;
              event.target.setSelectionRange(
                slash,
                dot > slash ? dot : event.target.value.length,
              );
            }}
          />
          {issue && !unchanged && <FieldError>{ISSUES[issue]()}</FieldError>}
        </Field>
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onClose}>
          {m['app.action.cancel']()}
        </Button>
        <Button type="submit" disabled={issue !== null || unchanged}>
          {prompt.action}
        </Button>
      </DialogFooter>
    </form>
  );
}
