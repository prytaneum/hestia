import { useState } from 'react';

import type { ServerTask, TaskAction } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { m } from '@/paraglide/messages.js';

type Timing = 'interval' | 'daily';
type Unit = 'm' | 'h' | 'd';

// Mirrors the daemon's floor on an interval.
const MIN_INTERVAL_SECONDS = 300;
const UNIT_SECONDS: Record<Unit, number> = { m: 60, h: 3600, d: 86_400 };

/** Compose a new scheduled task: what it does, and when. */
export function TaskDialog({
  open,
  pending,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (task: Partial<ServerTask>) => void;
}) {
  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      size="sm"
      title={m['server.tasks.add']()}
      description={m['server.tasks.add_description']()}
    >
      {open && (
        <TaskForm
          pending={pending}
          onCancel={() => onOpenChange(false)}
          onCreate={onCreate}
        />
      )}
    </AppDialog>
  );
}

function TaskForm({
  pending,
  onCancel,
  onCreate,
}: {
  pending: boolean;
  onCancel: () => void;
  onCreate: (task: Partial<ServerTask>) => void;
}) {
  const [action, setAction] = useState<TaskAction>('restart');
  const [command, setCommand] = useState('');
  const [timing, setTiming] = useState<Timing>('daily');
  const [time, setTime] = useState('04:00');
  const [count, setCount] = useState('6');
  const [unit, setUnit] = useState<Unit>('h');

  const actions = [
    { value: 'restart', label: m['server.tasks.action.restart']() },
    { value: 'command', label: m['server.tasks.action.command']() },
  ];
  const timings = [
    { value: 'daily', label: m['server.tasks.timing.daily']() },
    { value: 'interval', label: m['server.tasks.timing.interval']() },
  ];
  const units = [
    { value: 'm', label: m['entry.settings.interval.minutes']() },
    { value: 'h', label: m['entry.settings.interval.hours']() },
    { value: 'd', label: m['entry.settings.interval.days']() },
  ];

  const amount = Number(count);
  const intervalValid =
    /^\d+$/.test(count) && amount * UNIT_SECONDS[unit] >= MIN_INTERVAL_SECONDS;
  const valid =
    (action === 'restart' || command.trim() !== '') &&
    (timing === 'daily' ? /^\d{2}:\d{2}$/.test(time) : intervalValid);

  return (
    <form
      className="flex min-h-0 flex-1 flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (!valid) return;
        onCreate({
          action,
          command: action === 'command' ? command.trim() : '',
          schedule: timing === 'daily' ? time : `${amount}${unit}`,
          enabled: true,
        });
      }}
    >
      <DialogBody>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="task-action">
              {m['server.tasks.does']()}
            </FieldLabel>
            <Select
              items={actions}
              value={action}
              onValueChange={(v) => setAction((v ?? 'restart') as TaskAction)}
            >
              <SelectTrigger id="task-action" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {actions.map((item) => (
                  <SelectItem key={item.value} value={item.value}>
                    {item.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>

          {action === 'command' && (
            <Field>
              <FieldLabel htmlFor="task-command">
                {m['server.tasks.command']()}
              </FieldLabel>
              <Input
                id="task-command"
                className="font-mono"
                placeholder="say Restarting in 5 minutes"
                value={command}
                onChange={(event) => setCommand(event.target.value)}
              />
            </Field>
          )}

          <Field data-invalid={timing === 'interval' && !intervalValid}>
            <FieldLabel htmlFor="task-timing">
              {m['server.tasks.when']()}
            </FieldLabel>
            <div className="flex items-center gap-2">
              <Select
                items={timings}
                value={timing}
                onValueChange={(v) => setTiming((v ?? 'daily') as Timing)}
              >
                <SelectTrigger id="task-timing" className="flex-1">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {timings.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {timing === 'daily' ? (
                <Input
                  type="time"
                  aria-label={m['server.tasks.timing.daily']()}
                  className="w-28 font-mono"
                  value={time}
                  onChange={(event) => setTime(event.target.value)}
                />
              ) : (
                <>
                  <Input
                    type="number"
                    min={1}
                    aria-label={m['entry.settings.interval.every']()}
                    className="w-16"
                    value={count}
                    onChange={(event) => setCount(event.target.value)}
                  />
                  <Select
                    items={units}
                    value={unit}
                    onValueChange={(v) => setUnit((v ?? 'h') as Unit)}
                  >
                    <SelectTrigger
                      aria-label={m['entry.settings.interval.unit']()}
                      className="w-28"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {units.map((item) => (
                        <SelectItem key={item.value} value={item.value}>
                          {item.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </>
              )}
            </div>
            <FieldDescription>
              {timing === 'daily'
                ? m['server.tasks.timing.daily_hint']()
                : m['entry.settings.interval.too_short']()}
            </FieldDescription>
          </Field>
        </FieldGroup>
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCancel}>
          {m['app.action.cancel']()}
        </Button>
        <Button type="submit" disabled={!valid || pending}>
          {m['server.tasks.add']()}
        </Button>
      </DialogFooter>
    </form>
  );
}
