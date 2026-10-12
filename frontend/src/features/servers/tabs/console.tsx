import { TerminalWindowIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { type RefObject, useEffect, useMemo, useRef, useState } from 'react';

import { errorMessage } from '@/api';
import { Empty } from '@/components/empty';
import { type LogRow, LogView } from '@/components/log-view';
import { Input } from '@/components/ui/input';
import { m } from '@/paraglide/messages.js';
import {
  type ConsoleEntry,
  interleaveConsole,
  pushConsoleEntry,
  useConsoleHistory,
} from '@/queries/console';
import { serverMutations, useServerLogs } from '@/queries/server';

const rowClass: Record<ConsoleEntry['kind'], string | undefined> = {
  echo: 'text-foreground/70',
  reply: undefined,
  error: 'text-destructive',
};

/** Typing anywhere on the tab lands in the command line, as in a terminal. */
function useTypeToFocus(input: RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key.length !== 1) return;
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable ||
          target.closest('input, textarea, select, [role="dialog"]'))
      ) {
        return;
      }
      // Focusing during keydown routes this same keystroke into the input.
      input.current?.focus();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [input]);
}

export function ServerConsoleTab({
  id,
  running,
  name,
}: {
  id: string;
  running: boolean;
  name: string;
}) {
  // Followed whether or not it is running: the subject is the server, so a
  // stop leaves its output on screen and a restart streams into the same view.
  const logs = useServerLogs(id, { follow: true, tail: 500 });
  const command = useMutation(serverMutations.command(id));
  const [line, setLine] = useState('');
  const entries = useConsoleHistory(id);
  const input = useRef<HTMLInputElement>(null);
  useTypeToFocus(input);

  const lastLine = useRef<string | null>(null);
  lastLine.current = logs.lines.at(-1)?.line ?? null;

  const rows: LogRow[] = useMemo(
    () =>
      interleaveConsole(
        logs.lines.map((entry) => entry.line),
        entries,
      ).map((row) =>
        typeof row === 'string'
          ? { text: row }
          : { text: row.text, className: rowClass[row.kind] },
      ),
    [logs.lines, entries],
  );

  if (!running && logs.lines.length === 0 && entries.length === 0) {
    return (
      <Empty className="h-full" icon={TerminalWindowIcon}>
        {m['server.console.empty']()}
      </Empty>
    );
  }

  const push = (kind: ConsoleEntry['kind'], text: string) =>
    pushConsoleEntry(id, { kind, text, after: lastLine.current });

  const send = () => {
    const text = line.trim();
    if (!text) return;
    setLine('');
    push('echo', `» ${text}`);
    command.mutate(text, {
      onSuccess: (reply) => {
        const trimmed = reply.trim();
        if (trimmed) push('reply', trimmed);
      },
      onError: (error) => push('error', errorMessage(error)),
    });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <LogView
        rows={rows}
        emptyLabel={`${name} — ${m['app.status.online']()}`}
      />
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          send();
        }}
      >
        <Input
          ref={input}
          placeholder={
            running
              ? m['server.console.placeholder']()
              : m['server.console.empty']()
          }
          className="font-mono"
          value={line}
          onChange={(e) => setLine(e.target.value)}
          disabled={!running}
        />
      </form>
    </div>
  );
}
