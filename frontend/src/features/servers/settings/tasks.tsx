import { PlusIcon, XIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import type { ServerTask } from '@/api';
import {
  SettingGroup,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { TaskDialog } from '@/features/servers/dialogs';
import { m } from '@/paraglide/messages.js';
import { tasksMutations, tasksQueries } from '@/queries/tasks';

/** `04:30` reads as a daily time; anything else is an interval. */
function scheduleLabel(schedule: string): string {
  return schedule.includes(':')
    ? m['server.tasks.daily_at']({ time: schedule })
    : m['entry.settings.every_interval']({ interval: schedule });
}

/**
 * What the daemon runs for this server on a schedule: a restart, or a console
 * command. Backups keep their own schedule beside this one.
 */
export function TaskSettings({ id }: { id: string }) {
  const tasks = useQuery(tasksQueries.list(id));
  const save = useMutation(tasksMutations.save(id));
  const remove = useMutation(tasksMutations.remove(id));
  const [adding, setAdding] = useState(false);

  if (tasks.isPending) return <Bone className="h-24" />;
  const list = tasks.data ?? [];

  return (
    <SettingGroup
      legend={m['server.tasks.title']()}
      description={m['server.tasks.description']()}
    >
      {list.length > 0 && (
        <SettingList>
          {list.map((task: ServerTask) => (
            <SettingRow
              key={task.id}
              htmlFor={`task-${task.id}`}
              label={
                task.action === 'restart' ? (
                  m['server.tasks.action.restart']()
                ) : (
                  <span className="truncate font-mono">{task.command}</span>
                )
              }
              description={scheduleLabel(task.schedule)}
            >
              <Switch
                id={`task-${task.id}`}
                size="sm"
                checked={task.enabled}
                disabled={save.isPending}
                onCheckedChange={(enabled) =>
                  save.mutate({ ...task, enabled: enabled === true })
                }
              />
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={m['app.action.remove']()}
                disabled={remove.isPending}
                onClick={() => remove.mutate(task.id)}
              >
                <XIcon className="size-3.5" />
              </Button>
            </SettingRow>
          ))}
        </SettingList>
      )}
      <div>
        <Button
          variant="outline"
          size="sm"
          data-icon="inline-start"
          onClick={() => setAdding(true)}
        >
          <PlusIcon weight="bold" />
          {m['server.tasks.add']()}
        </Button>
      </div>

      <TaskDialog
        open={adding}
        pending={save.isPending}
        onOpenChange={setAdding}
        onCreate={(task) =>
          save.mutate(task, { onSuccess: () => setAdding(false) })
        }
      />
    </SettingGroup>
  );
}
