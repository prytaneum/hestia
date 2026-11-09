import { TerminalWindowIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { type ReactNode, useMemo, useState } from 'react';

import type { CommandArg, CommandScope, EntryKind, PluginCommand } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import {
  DropdownMenuItem,
  DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { m } from '@/paraglide/messages.js';
import { runJobMutation } from '@/queries/jobs';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

type Value = string | number | boolean;

const SCOPE: Record<EntryKind, CommandScope> = {
  server: 'server',
  instance: 'instance',
};

/** What a command acts on: an entry for a scoped command, nothing for a top one. */
export type CommandTarget =
  | { kind: EntryKind; id: string }
  | { plugin: string };

function matches(command: PluginCommand, target: CommandTarget): boolean {
  if ('plugin' in target) {
    return command.plugin === target.plugin && command.command.scope === 'top';
  }
  return command.command.scope === SCOPE[target.kind];
}

function initial(args: CommandArg[]): Record<string, Value> {
  return Object.fromEntries(
    args.map((arg) => [
      arg.name,
      arg.kind === 'bool' ? false : arg.kind === 'number' ? 0 : '',
    ]),
  );
}

/** Drops the optional text a person left empty, so the plugin sees it as absent. */
function given(
  args: CommandArg[],
  values: Record<string, Value>,
): Record<string, unknown> {
  return Object.fromEntries(
    args
      .filter((arg) => arg.required || values[arg.name] !== '')
      .map((arg) => [arg.name, values[arg.name]]),
  );
}

function ArgumentsDialog({
  command,
  onRun,
  onClose,
}: {
  command: PluginCommand;
  onRun: (args: Record<string, unknown>) => void;
  onClose: () => void;
}) {
  const { args, title, description } = command.command;
  const [values, setValues] = useState(() => initial(args));
  const set = (name: string, value: Value) =>
    setValues((previous) => ({ ...previous, [name]: value }));
  const complete = args.every(
    (arg) => !arg.required || arg.kind === 'bool' || values[arg.name] !== '',
  );

  return (
    <AppDialog
      open
      onOpenChange={(open) => !open && onClose()}
      icon={TerminalWindowIcon}
      title={title || command.command.id}
      description={description || undefined}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (complete) onRun(given(args, values));
        }}
      >
        <DialogBody>
          <FieldGroup>
            {args.map((arg) => (
              <Field key={arg.name}>
                <FieldLabel htmlFor={`arg-${arg.name}`}>
                  {arg.name}
                  {arg.required && ' *'}
                </FieldLabel>
                {arg.kind === 'bool' ? (
                  <Switch
                    id={`arg-${arg.name}`}
                    checked={values[arg.name] === true}
                    onCheckedChange={(next) => set(arg.name, next === true)}
                  />
                ) : (
                  <Input
                    id={`arg-${arg.name}`}
                    type={arg.kind === 'number' ? 'number' : 'text'}
                    value={String(values[arg.name])}
                    onChange={(event) =>
                      set(
                        arg.name,
                        arg.kind === 'number'
                          ? event.target.valueAsNumber || 0
                          : event.target.value,
                      )
                    }
                  />
                )}
                {arg.help && <FieldDescription>{arg.help}</FieldDescription>}
              </Field>
            ))}
          </FieldGroup>
        </DialogBody>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            {m['app.action.cancel']()}
          </Button>
          <Button type="submit" disabled={!complete}>
            {m['plugins.command.run']()}
          </Button>
        </DialogFooter>
      </form>
    </AppDialog>
  );
}

/**
 * The plugin commands that act on `target`, as menu items plus the dialog that
 * asks for their arguments. A command without declared arguments runs at once;
 * the job store reports its progress and its outcome.
 */
export function usePluginCommands(target: CommandTarget): {
  commands: PluginCommand[];
  run: (command: PluginCommand) => void;
  dialog: ReactNode;
} {
  const { data } = useQuery(pluginQueries.commands());
  const [asking, setAsking] = useState<PluginCommand | null>(null);
  const commands = useMemo(
    () => (data ?? []).filter((command) => matches(command, target)),
    [data, target],
  );

  const start = (command: PluginCommand, args: Record<string, unknown>) =>
    runJobMutation(pluginMutations.run(), {
      plugin: command.plugin,
      command: command.command.id,
      title: command.command.title || command.command.id,
      entry: 'plugin' in target ? undefined : target,
      args,
    });

  return {
    commands,
    run: (command) =>
      command.command.args.length > 0 ? setAsking(command) : start(command, {}),
    dialog: asking && (
      <ArgumentsDialog
        command={asking}
        onClose={() => setAsking(null)}
        onRun={(args) => {
          setAsking(null);
          start(asking, args);
        }}
      />
    ),
  };
}

/** The entry's plugin commands for its actions menu, after a separator. */
export function PluginCommandItems({
  commands,
  run,
}: {
  commands: PluginCommand[];
  run: (command: PluginCommand) => void;
}) {
  if (commands.length === 0) return null;
  return (
    <>
      <DropdownMenuSeparator />
      {commands.map((command) => (
        <DropdownMenuItem
          key={`${command.plugin}/${command.command.id}`}
          title={command.command.description || undefined}
          onClick={() => run(command)}
        >
          <TerminalWindowIcon />
          {command.command.title || command.command.id}
        </DropdownMenuItem>
      ))}
    </>
  );
}
