import { TerminalWindowIcon } from '@phosphor-icons/react';
import { useMutation } from '@tanstack/react-query';
import { useMemo, useRef } from 'react';

import { errorMessage, type ProcessLogLine } from '@/api';
import { Empty } from '@/components/empty';
import { type LogRow, LogView } from '@/components/log-view';
import { m } from '@/paraglide/messages.js';
import {
  type ConsoleEntry,
  clearConsole,
  interleaveConsole,
  pushConsoleEntry,
  useConsoleHistory,
  useUnclearedLines,
} from '@/queries/console';
import { serverMutations, useServerLogs } from '@/queries/server';
import { ConsoleInput } from './console-input';

const rowClass: Record<ConsoleEntry['kind'], string | undefined> = {
  echo: 'text-foreground/70',
  reply: undefined,
  error: 'text-destructive',
};

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
  const entries = useConsoleHistory(id);

  const lines = useUnclearedLines(id, logs.lines);
  const lastLine = useRef<ProcessLogLine | null>(null);
  lastLine.current = lines.at(-1) ?? null;

  const rows: LogRow[] = useMemo(
    () =>
      interleaveConsole(lines, entries).map((row) =>
        'kind' in row
          ? { text: row.text, className: rowClass[row.kind] }
          : { text: row.line },
      ),
    [lines, entries],
  );

  if (!running && lines.length === 0 && entries.length === 0) {
    return (
      <Empty className="h-full" icon={TerminalWindowIcon}>
        {m['server.console.empty']()}
      </Empty>
    );
  }

  const push = (kind: ConsoleEntry['kind'], text: string) =>
    pushConsoleEntry(id, { kind, text, after: lastLine.current });

  const send = (text: string) => {
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
      <ConsoleInput
        id={id}
        running={running}
        onSend={send}
        onClear={() => clearConsole(id, logs.lines.at(-1) ?? null)}
      />
    </div>
  );
}
