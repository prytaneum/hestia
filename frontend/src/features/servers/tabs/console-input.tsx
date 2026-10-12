import { useQuery } from '@tanstack/react-query';
import {
  type KeyboardEvent,
  type RefObject,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react';

import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { consoleCommands, pushConsoleCommand } from '@/queries/console';
import { serverQueries } from '@/queries/server';

const LAST_WORD = /\S*$/;

/** Typing anywhere on the tab lands in the command line, as in a terminal. */
function useTypeToFocus(
  input: RefObject<HTMLInputElement | null>,
  onType: (character: string) => void,
) {
  const type = useRef(onType);
  type.current = onType;

  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
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
      const field = input.current;
      if (!field || field.disabled) return;
      // The character is inserted here rather than left to the keystroke:
      // webviews differ on whether a focus change during keydown redirects it.
      event.preventDefault();
      field.focus();
      type.current(event.key);
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [input]);
}

export function ConsoleInput({
  id,
  running,
  onSend,
}: {
  id: string;
  running: boolean;
  onSend: (command: string) => void;
}) {
  const [line, setLine] = useState('');
  const [cycle, setCycle] = useState<{ words: string[]; at: number } | null>(
    null,
  );
  const [recall, setRecall] = useState<{ at: number; draft: string } | null>(
    null,
  );
  const [dismissed, setDismissed] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();

  const edit = (next: string | ((typed: string) => string)) => {
    setLine(next);
    setCycle(null);
    setRecall(null);
    setDismissed(false);
  };
  useTypeToFocus(input, (character) => edit((typed) => typed + character));

  const asking = running && line.trim() !== '' && !cycle && !recall;
  const offered = useQuery({
    ...serverQueries.complete(id, line),
    enabled: asking,
  });
  const found = asking ? offered.data : undefined;
  const typed = LAST_WORD.exec(line)?.[0] ?? '';
  const suggested = found?.completions ?? [];
  const words =
    cycle?.words ??
    (suggested.length === 1 && suggested[0] === typed ? [] : suggested);
  const hints = cycle ? [] : (found?.arguments ?? []);
  const open = !dismissed && (words.length > 0 || hints.length > 0);
  const stale = !cycle && offered.isPlaceholderData;

  const choose = (at: number) => {
    setLine(line.replace(LAST_WORD, () => words[at]));
    setCycle({ words, at });
  };

  const step = (delta: 1 | -1) => {
    const first = delta > 0 ? 0 : words.length - 1;
    choose(cycle ? (cycle.at + delta + words.length) % words.length : first);
  };

  const recallStep = (delta: 1 | -1) => {
    const sent = consoleCommands(id);
    const at = recall ? recall.at + delta : sent.length - 1;
    if ((!recall && delta > 0) || at < 0) return;
    setCycle(null);
    if (at >= sent.length) {
      setLine(recall?.draft ?? '');
      setRecall(null);
      return;
    }
    setLine(sent[at]);
    setRecall({ at, draft: recall?.draft ?? line });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const cycling = open && words.length > 0 && !stale;
    if (event.key === 'Tab' && cycling) {
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      if (cycling) step(delta);
      else recallStep(delta);
    } else if (event.key === 'Escape' && open) {
      event.stopPropagation();
      setCycle(null);
      setDismissed(true);
    }
  };

  const send = () => {
    const command = line.trim();
    if (!command) return;
    pushConsoleCommand(id, command);
    edit('');
    onSend(command);
  };

  return (
    <form
      className="relative flex gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      {open && (
        <div
          id={listId}
          role="listbox"
          className="absolute bottom-full left-0 z-50 mb-1 max-h-48 max-w-full min-w-48 overflow-y-auto bg-popover font-mono text-xs text-popover-foreground shadow-md ring-1 ring-foreground/10"
        >
          {words.map((word, at) => (
            <div
              key={word}
              id={`${listId}-${at}`}
              role="option"
              tabIndex={-1}
              aria-selected={cycle?.at === at}
              ref={
                cycle?.at === at
                  ? (row) => row?.scrollIntoView({ block: 'nearest' })
                  : undefined
              }
              className={cn(
                'cursor-default px-2 py-1 hover:bg-accent hover:text-accent-foreground',
                cycle?.at === at && 'bg-accent text-accent-foreground',
              )}
              // Mouse down, not click: a click would take focus off the input.
              onMouseDown={(event) => {
                event.preventDefault();
                choose(at);
              }}
            >
              {word}
            </div>
          ))}
          {hints.length > 0 && (
            <div className="px-2 py-1 text-muted-foreground">
              {hints.join(' ')}
            </div>
          )}
        </div>
      )}
      <Input
        ref={input}
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-activedescendant={cycle ? `${listId}-${cycle.at}` : undefined}
        placeholder={
          running
            ? m['server.console.placeholder']()
            : m['server.console.empty']()
        }
        className="font-mono"
        value={line}
        onChange={(event) => edit(event.target.value)}
        onKeyDown={onKeyDown}
        disabled={!running}
      />
    </form>
  );
}
