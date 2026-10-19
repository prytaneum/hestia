import { ArrowLeftIcon, FileXIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { lazy, Suspense, useState } from 'react';
import { errorMessage } from '@/api';
import { Empty } from '@/components/empty';
import { SaveButton, SaveStatus } from '@/components/save-button';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { baseName, editorLanguage } from '@/features/servers/lib';
import { useSaveState } from '@/hooks/use-save-state';
import { m } from '@/paraglide/messages.js';
import { filesMutations, filesQueries } from '@/queries/files';

// CodeMirror is only paid for once a file is actually opened.
const CodeEditor = lazy(() => import('./code-editor'));

/** One text file, open for editing. Save writes it whole. */
export function FileEditor({
  id,
  path,
  running,
  onClose,
}: {
  id: string;
  path: string;
  running: boolean;
  onClose: () => void;
}) {
  const content = useQuery(filesQueries.content(id, path));
  const write = useMutation(filesMutations.write(id));
  const { state, run } = useSaveState();
  const [draft, setDraft] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [leaving, setLeaving] = useState(false);

  const stored = saved ?? content.data ?? '';
  const dirty = draft !== null && draft !== stored;

  const save = () => {
    if (draft === null || draft === stored) return;
    const text = draft;
    void run(() =>
      write.mutateAsync({ path, content: text }).then(() => setSaved(text)),
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={m['app.action.back']()}
          onClick={() => (dirty ? setLeaving(true) : onClose())}
        >
          <ArrowLeftIcon />
        </Button>
        <div className="min-w-0 flex-1">
          <div className="truncate font-mono text-xs">{baseName(path)}</div>
          <div className="truncate font-mono text-[11px] text-muted-foreground">
            {path}
          </div>
        </div>
        {dirty && (
          <span className="text-[11px] text-amber">
            {m['server.files.editor.unsaved']()}
          </span>
        )}
        <SaveStatus state={state} />
        <SaveButton
          state={state}
          disabled={!dirty && state === 'idle'}
          onClick={save}
        />
      </div>

      {content.isPending ? (
        <Bone className="flex-1" />
      ) : content.isError ? (
        <Empty className="flex-1" icon={FileXIcon}>
          {errorMessage(content.error)}
        </Empty>
      ) : (
        <div className="min-h-0 flex-1 border border-border">
          <Suspense fallback={<Bone className="h-full" />}>
            <CodeEditor
              key={path}
              value={content.data}
              language={editorLanguage(baseName(path))}
              onChange={setDraft}
              onSave={save}
            />
          </Suspense>
        </div>
      )}
      {running && (
        <p className="text-[11px] text-muted-foreground">
          {m['server.files.editor.running_hint']()}
        </p>
      )}

      <ConfirmDialog
        open={leaving}
        onOpenChange={setLeaving}
        title={m['server.files.editor.discard_title']()}
        description={m['server.files.editor.discard_description']({
          name: baseName(path),
        })}
        destructive
        confirmLabel={m['server.files.editor.discard']()}
        onConfirm={onClose}
      />
    </div>
  );
}
